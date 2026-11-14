// Command rterm-dmg assembles rterm.app from one or two Mach-O slices and
// builds the styled installer DMG. It runs on Linux, macOS, and Windows: the
// HFS+ volume, the UDIF container, and the Finder .DS_Store are written in Go.
package main

import (
	"bytes"
	"debug/macho"
	"encoding/binary"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/leaanthony/dmg/dmg"
	"github.com/leaanthony/dmg/icns"
)

const (
	windowWidth  = 600
	windowHeight = 400
	appIconX     = 170
	appIconY     = 186
	appsIconX    = 430
	appsIconY    = 186
	iconSize     = 128

	// fatAlignShift is the Mach-O fat slice alignment (2^14), what Apple's lipo
	// uses for arm64 slices.
	fatAlignShift = 14
	fatAlign      = 1 << fatAlignShift
)

// config is everything the packager needs. The shell wrapper resolves paths
// and the version; this program owns bundle and DMG assembly.
type config struct {
	Version    string
	Output     string
	Arm64      string
	AMD64      string
	Stage      string
	Plist      string
	Icon       string
	AppName    string
	VolumeName string
	// AllowNonMachO accepts a single non-Mach-O executable. Only the packaging
	// smoke test uses it.
	AllowNonMachO bool
}

func main() {
	var cfg config
	flag.StringVar(&cfg.Version, "version", "", "bundle and DMG version")
	flag.StringVar(&cfg.Output, "output", "", "DMG output path")
	flag.StringVar(&cfg.Arm64, "arm64", "", "arm64 Mach-O binary")
	flag.StringVar(&cfg.AMD64, "amd64", "", "x86_64 Mach-O binary")
	flag.StringVar(&cfg.Stage, "stage", "build/dmg", "staging directory")
	flag.StringVar(&cfg.Plist, "plist", "packaging/Info.plist", "Info.plist template")
	flag.StringVar(&cfg.Icon, "icon", "packaging/icon.png", "source app icon")
	flag.StringVar(&cfg.AppName, "app-name", "rterm", "app bundle and volume name")
	flag.StringVar(&cfg.VolumeName, "volume-name", "rterm", "mounted volume name")
	flag.BoolVar(&cfg.AllowNonMachO, "allow-non-macho", false, "accept a non-Mach-O executable (smoke test only)")
	flag.Parse()

	app, err := build(cfg)
	if err != nil {
		fmt.Fprintln(os.Stderr, "rterm-dmg:", err)
		os.Exit(1)
	}
	fmt.Println("rterm-dmg: app", app)
	fmt.Println("rterm-dmg: dmg", cfg.Output)
}

// build writes the app bundle and the DMG, and returns the bundle path.
func build(cfg config) (string, error) {
	if cfg.Version == "" {
		return "", fmt.Errorf("version is required")
	}
	if strings.ContainsAny(cfg.Version, " \t/") {
		return "", fmt.Errorf("version %q is not a plain tag", cfg.Version)
	}
	if cfg.Output == "" {
		return "", fmt.Errorf("output is required")
	}
	if cfg.AppName == "" || cfg.VolumeName == "" {
		return "", fmt.Errorf("app-name and volume-name are required")
	}
	if cfg.Stage == "" || cfg.Stage == "." || cfg.Stage == "/" {
		return "", fmt.Errorf("stage must be a scratch directory")
	}

	app := filepath.Join(cfg.Stage, cfg.AppName+".app")
	contents := filepath.Join(app, "Contents")
	resources := filepath.Join(contents, "Resources")
	if err := os.RemoveAll(cfg.Stage); err != nil {
		return "", err
	}
	for _, dir := range []string{
		filepath.Join(contents, "MacOS"),
		resources,
		filepath.Dir(cfg.Output),
	} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return "", err
		}
	}

	exe, err := universalBinary(cfg.Arm64, cfg.AMD64, cfg.AllowNonMachO)
	if err != nil {
		return "", err
	}
	if err := os.WriteFile(filepath.Join(contents, "MacOS", cfg.AppName), exe, 0o755); err != nil {
		return "", err
	}

	plist, err := os.ReadFile(cfg.Plist)
	if err != nil {
		return "", fmt.Errorf("read plist: %w", err)
	}
	stamped := strings.ReplaceAll(string(plist), "__VERSION__", cfg.Version)
	if err := os.WriteFile(filepath.Join(contents, "Info.plist"), []byte(stamped), 0o644); err != nil {
		return "", err
	}
	if err := os.WriteFile(filepath.Join(contents, "PkgInfo"), []byte("APPL????"), 0o644); err != nil {
		return "", err
	}

	appIcon := filepath.Join(resources, "AppIcon.icns")
	if err := icns.EncodeFile(appIcon, cfg.Icon); err != nil {
		return "", fmt.Errorf("encode icns: %w", err)
	}

	background := filepath.Join(cfg.Stage, "background.png")
	if err := installerBackground(windowWidth, windowHeight, background); err != nil {
		return "", fmt.Errorf("draw background: %w", err)
	}

	err = dmg.Build(dmg.Options{
		VolumeName:             cfg.VolumeName,
		OutputPath:             cfg.Output,
		Files:                  map[string]string{cfg.AppName + ".app": app},
		AddApplicationsSymlink: true,
		Backend:                dmg.BackendNative,
		IconPositions: map[string]dmg.IconPosition{
			cfg.AppName + ".app": {X: appIconX, Y: appIconY},
			"Applications":       {X: appsIconX, Y: appsIconY},
		},
		Window: dmg.WindowConfig{X: 200, Y: 160, Width: windowWidth, Height: windowHeight},
		Icon:   dmg.IconConfig{Size: iconSize, TextSize: 13, GridSpace: 100},
		Background: &dmg.BackgroundConfig{
			File: background,
		},
		VolumeIcon: appIcon,
	})
	if err != nil {
		return "", fmt.Errorf("build dmg: %w", err)
	}
	return app, nil
}

// universalBinary returns one Mach-O slice as-is, or a fat Mach-O when both
// architectures are present.
func universalBinary(arm64Path, amd64Path string, allowNonMachO bool) ([]byte, error) {
	var paths []string
	for _, p := range []string{arm64Path, amd64Path} {
		if p != "" {
			paths = append(paths, p)
		}
	}
	if len(paths) == 0 {
		return nil, fmt.Errorf("no Mach-O binary given")
	}
	if len(paths) == 1 {
		data, err := os.ReadFile(paths[0])
		if err != nil {
			return nil, err
		}
		if !allowNonMachO {
			if _, err := macho.NewFile(bytes.NewReader(data)); err != nil {
				return nil, fmt.Errorf("%s is not a Mach-O binary: %w", paths[0], err)
			}
		}
		return data, nil
	}
	return fatBinary(paths)
}

type slice struct {
	cpu  uint32
	sub  uint32
	data []byte
}

// fatBinary merges Mach-O files into a universal binary. The layout follows
// Apple's fat_arch: big-endian header, 2^14 aligned slices.
func fatBinary(paths []string) ([]byte, error) {
	slices := make([]slice, 0, len(paths))
	seen := map[uint32]string{}
	for _, path := range paths {
		f, err := macho.Open(path)
		if err != nil {
			return nil, fmt.Errorf("read Mach-O %s: %w", path, err)
		}
		cpu, sub := uint32(f.Cpu), uint32(f.SubCpu)
		f.Close()
		if other, ok := seen[cpu]; ok {
			return nil, fmt.Errorf("%s and %s are both %s", other, path, f.Cpu)
		}
		seen[cpu] = path
		data, err := os.ReadFile(path)
		if err != nil {
			return nil, err
		}
		slices = append(slices, slice{cpu: cpu, sub: sub, data: data})
	}

	offsets := make([]uint32, len(slices))
	offset := align(uint32(8 + 20*len(slices)))
	for i, s := range slices {
		offsets[i] = offset
		offset += align(uint32(len(s.data)))
	}

	var out bytes.Buffer
	write := func(v uint32) {
		var b [4]byte
		binary.BigEndian.PutUint32(b[:], v)
		out.Write(b[:])
	}
	write(0xcafebabe)
	write(uint32(len(slices)))
	for i, s := range slices {
		write(s.cpu)
		write(s.sub)
		write(offsets[i])
		write(uint32(len(s.data)))
		write(fatAlignShift)
	}
	for i, s := range slices {
		if pad := int(offsets[i]) - out.Len(); pad > 0 {
			out.Write(make([]byte, pad))
		}
		out.Write(s.data)
	}

	merged := out.Bytes()
	if _, err := macho.NewFatFile(bytes.NewReader(merged)); err != nil {
		return nil, fmt.Errorf("verify fat binary: %w", err)
	}
	return merged, nil
}

func align(n uint32) uint32 {
	return (n + fatAlign - 1) &^ (fatAlign - 1)
}
