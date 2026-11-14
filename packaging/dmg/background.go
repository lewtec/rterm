package main

import (
	"image"
	"image/color"
	"image/png"
	"math"
	"os"
)

// Finder draws a background picture at one image pixel per point, anchored at
// the window's top-left corner; it does not scale the image to fit the window.
// The image must therefore be exactly the window's point size. Drawing it at
// 2x (the obvious Retina choice) makes Finder render everything at double size
// and push the lower half of the design, label plates included, out of view.
const backgroundScale = 1

// palette matches the app icon: near-black navy with a cyan glow.
var (
	bgTop     = color.RGBA{R: 0x16, G: 0x19, B: 0x20, A: 0xff}
	bgBottom  = color.RGBA{R: 0x24, G: 0x2a, B: 0x35, A: 0xff}
	bgAccent  = color.RGBA{R: 0x4d, G: 0xc3, B: 0xf0, A: 0xff}
	bgArrow   = color.RGBA{R: 0xf4, G: 0xf7, B: 0xfb, A: 0xff}
	bgPlate   = color.RGBA{R: 0xff, G: 0xff, B: 0xff, A: 0xff}
	bgPlateAa = 0.80
)

// labelPlate is the light chip baked under each icon, where Finder draws the
// item's name. A background picture makes Finder render names in black in both
// light and dark themes, and there is no label-colour setting in .DS_Store, so
// the chip is what makes the names readable on the dark surface.
var labelPlate = struct {
	width, height, top, radius int
}{
	width:  iconSize,
	height: 30,
	top:    appIconY + iconSize/2,
	radius: 15,
}

// installerBackground renders the styled background and writes it as a PNG.
func installerBackground(windowWidth, windowHeight int, path string) error {
	w := windowWidth * backgroundScale
	h := windowHeight * backgroundScale
	img := image.NewRGBA(image.Rect(0, 0, w, h))

	verticalGradient(img, bgTop, bgBottom)
	// Cyan light behind the app icon, echoing the glow in the app icon itself.
	radialAdd(img, appIconX*backgroundScale, appIconY*backgroundScale,
		float64(200*backgroundScale), bgAccent, 0.18)
	// Neutral light under the Applications folder so the pair feels balanced.
	radialAdd(img, appsIconX*backgroundScale, appsIconY*backgroundScale,
		float64(185*backgroundScale), color.RGBA{R: 0xff, G: 0xff, B: 0xff, A: 0xff}, 0.05)

	dotGrid(img, 22*backgroundScale, 0.9*backgroundScale,
		color.RGBA{R: 0xff, G: 0xff, B: 0xff, A: 0xff}, 0.030)
	vignette(img, 0.22)

	drawArrow(img)
	labelPlates(img)

	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	if err := png.Encode(f, img); err != nil {
		return err
	}
	return f.Close()
}

// verticalGradient fills img with a dithered vertical gradient. The dither keeps
// large flat areas from banding in 8-bit PNG.
func verticalGradient(img *image.RGBA, from, to color.RGBA) {
	b := img.Bounds()
	h := b.Dy()
	for y := 0; y < h; y++ {
		t := float64(y) / float64(h-1)
		row := img.Pix[y*img.Stride : y*img.Stride+b.Dx()*4]
		for x := 0; x < b.Dx(); x++ {
			d := bayer8(x, y) / 64.0 // 0..1
			i := x * 4
			row[i+0] = lerp8(from.R, to.R, t, d)
			row[i+1] = lerp8(from.G, to.G, t, d)
			row[i+2] = lerp8(from.B, to.B, t, d)
			row[i+3] = 0xff
		}
	}
}

func lerp8(a, b uint8, t, d float64) uint8 {
	v := float64(a)*(1-t) + float64(b)*t
	v += (d - 0.5) * 1.2 // ordered dither, below one 8-bit step
	if v < 0 {
		v = 0
	}
	if v > 255 {
		v = 255
	}
	return uint8(v + 0.5)
}

// bayer8 is the classic 8x8 Bayer threshold matrix.
func bayer8(x, y int) float64 {
	m := [8][8]int{
		{0, 32, 8, 40, 2, 34, 10, 42},
		{48, 16, 56, 24, 50, 18, 58, 26},
		{12, 44, 4, 36, 14, 46, 6, 38},
		{60, 28, 52, 20, 62, 30, 54, 22},
		{3, 35, 11, 43, 1, 33, 9, 41},
		{51, 19, 59, 27, 49, 17, 57, 25},
		{15, 47, 7, 39, 13, 45, 5, 37},
		{63, 31, 55, 23, 61, 29, 53, 21},
	}
	return float64(m[y%8][x%8])
}

// radialAdd adds a soft circular light with a gaussian falloff.
func radialAdd(img *image.RGBA, cx, cy int, radius float64, c color.RGBA, peak float64) {
	b := img.Bounds()
	inv := 1 / (radius * radius)
	for y := b.Min.Y; y < b.Max.Y; y++ {
		dy := float64(y - cy)
		for x := b.Min.X; x < b.Max.X; x++ {
			dx := float64(x - cx)
			d2 := (dx*dx + dy*dy) * inv
			if d2 >= 1 {
				continue
			}
			// Smooth, gaussian-ish falloff, zero at the edge.
			a := peak * math.Exp(-3.2*d2) * (1 - d2)
			if a <= 0.001 {
				continue
			}
			i := y*img.Stride + x*4
			img.Pix[i+0] = add8(img.Pix[i+0], float64(c.R)*a)
			img.Pix[i+1] = add8(img.Pix[i+1], float64(c.G)*a)
			img.Pix[i+2] = add8(img.Pix[i+2], float64(c.B)*a)
		}
	}
}

func add8(base uint8, v float64) uint8 {
	f := float64(base) + v
	if f > 255 {
		return 255
	}
	return uint8(f)
}

// dotGrid draws a faint grid of dots for texture.
func dotGrid(img *image.RGBA, spacing, radius float64, c color.RGBA, alpha float64) {
	b := img.Bounds()
	for y := spacing / 2; y < float64(b.Dy()); y += spacing {
		for x := spacing / 2; x < float64(b.Dx()); x += spacing {
			fillDisc(img, x, y, radius, c, alpha)
		}
	}
}

// fillDisc draws an anti-aliased filled circle.
func fillDisc(img *image.RGBA, cx, cy, radius float64, c color.RGBA, alpha float64) {
	r := radius + 0.5
	x0, y0 := int(math.Floor(cx-r)), int(math.Floor(cy-r))
	x1, y1 := int(math.Ceil(cx+r)), int(math.Ceil(cy+r))
	for y := y0; y <= y1; y++ {
		for x := x0; x <= x1; x++ {
			dx, dy := float64(x)-cx, float64(y)-cy
			cov := clamp01(r - math.Hypot(dx, dy))
			if cov > 0 {
				blend(img, x, y, c, alpha*cov)
			}
		}
	}
}

// strokePolyline draws an anti-aliased round-capped, round-joined line.
func strokePolyline(img *image.RGBA, pts [][2]float64, width float64, c color.RGBA, alpha float64) {
	half := width / 2
	pad := half + 1
	minX, minY := pts[0][0], pts[0][1]
	maxX, maxY := minX, minY
	for _, p := range pts {
		minX, minY = math.Min(minX, p[0]), math.Min(minY, p[1])
		maxX, maxY = math.Max(maxX, p[0]), math.Max(maxY, p[1])
	}
	for y := int(math.Floor(minY - pad)); y <= int(math.Ceil(maxY+pad)); y++ {
		for x := int(math.Floor(minX - pad)); x <= int(math.Ceil(maxX+pad)); x++ {
			px, py := float64(x)+0.5, float64(y)+0.5
			best := math.Inf(1)
			for i := 0; i+1 < len(pts); i++ {
				d := distToSegment(px, py, pts[i], pts[i+1])
				if d < best {
					best = d
				}
			}
			cov := clamp01(half + 0.5 - best)
			if cov > 0 {
				blend(img, x, y, c, alpha*cov)
			}
		}
	}
}

func distToSegment(px, py float64, a, b [2]float64) float64 {
	dx, dy := b[0]-a[0], b[1]-a[1]
	len2 := dx*dx + dy*dy
	t := 0.0
	if len2 > 0 {
		t = ((px-a[0])*dx + (py-a[1])*dy) / len2
		t = math.Max(0, math.Min(1, t))
	}
	return math.Hypot(px-(a[0]+t*dx), py-(a[1]+t*dy))
}

// drawArrow draws a soft cyan halo with a crisp chevron on top, pointing from
// the app icon to the Applications drop link.
func drawArrow(img *image.RGBA) {
	s := float64(backgroundScale)
	y := float64(appIconY) * s
	shaftStart := float64(appIconX+iconSize/2+22) * s
	tipX := float64(appsIconX-iconSize/2-26) * s
	head := 15 * s
	headSpread := 13 * s
	chevron := [][2]float64{
		{shaftStart, y},
		{tipX, y},
	}
	headPath := [][2]float64{
		{tipX - head, y - headSpread},
		{tipX, y},
		{tipX - head, y + headSpread},
	}
	// Halo first, then the crisp stroke.
	for _, h := range []struct {
		w     float64
		alpha float64
	}{
		{20 * s, 0.04},
		{11 * s, 0.07},
	} {
		strokePolyline(img, chevron, h.w, bgAccent, h.alpha)
		strokePolyline(img, headPath, h.w, bgAccent, h.alpha)
	}
	strokePolyline(img, chevron, 4*s, bgArrow, 0.60)
	strokePolyline(img, headPath, 4*s, bgArrow, 0.60)
}

// labelPlates bakes the light chips under both icons so Finder's black item
// names have contrast on the dark surface.
func labelPlates(img *image.RGBA) {
	s := float64(backgroundScale)
	w := float64(labelPlate.width) * s
	h := float64(labelPlate.height) * s
	top := (float64(labelPlate.top) + float64(labelPlate.height)/2) * s
	for _, cx := range []int{appIconX, appsIconX} {
		roundedPlate(img, float64(cx)*s, top, w, h, float64(labelPlate.radius)*s, bgPlate, bgPlateAa)
	}
}

// roundedPlate fills an anti-aliased rounded rectangle centred at (cx, cy).
func roundedPlate(img *image.RGBA, cx, cy, w, h, radius float64, c color.RGBA, alpha float64) {
	x0, y0 := int(cx-w/2-1), int(cy-h/2-1)
	x1, y1 := int(cx+w/2+1), int(cy+h/2+2)
	hx, hy := w/2-radius, h/2-radius
	for y := y0; y <= y1; y++ {
		for x := x0; x <= x1; x++ {
			qx := math.Abs(float64(x)+0.5-cx) - hx
			qy := math.Abs(float64(y)+0.5-cy) - hy
			d := math.Hypot(math.Max(qx, 0), math.Max(qy, 0)) + math.Min(math.Max(qx, qy), 0) - radius
			cov := clamp01(0.5 - d)
			if cov > 0 {
				blend(img, x, y, c, alpha*cov)
			}
		}
	}
}

// vignette darkens the corners so the icons stay the visual focus.
func vignette(img *image.RGBA, strength float64) {
	b := img.Bounds()
	cx, cy := float64(b.Dx())/2, float64(b.Dy())/2
	maxR := math.Hypot(cx, cy)
	for y := b.Min.Y; y < b.Max.Y; y++ {
		for x := b.Min.X; x < b.Max.X; x++ {
			d := math.Hypot(float64(x)-cx, float64(y)-cy) / maxR
			t := clamp01((d - 0.45) / 0.55)
			if t == 0 {
				continue
			}
			f := 1 - strength*t*t
			i := y*img.Stride + x*4
			img.Pix[i+0] = uint8(float64(img.Pix[i+0]) * f)
			img.Pix[i+1] = uint8(float64(img.Pix[i+1]) * f)
			img.Pix[i+2] = uint8(float64(img.Pix[i+2]) * f)
		}
	}
}

// blend composites c over the pixel at (x, y) with the given alpha.
func blend(img *image.RGBA, x, y int, c color.RGBA, alpha float64) {
	if alpha <= 0 {
		return
	}
	if alpha > 1 {
		alpha = 1
	}
	b := img.Bounds()
	if x < b.Min.X || x >= b.Max.X || y < b.Min.Y || y >= b.Max.Y {
		return
	}
	i := y*img.Stride + x*4
	img.Pix[i+0] = mix8(img.Pix[i+0], c.R, alpha)
	img.Pix[i+1] = mix8(img.Pix[i+1], c.G, alpha)
	img.Pix[i+2] = mix8(img.Pix[i+2], c.B, alpha)
}

func mix8(base, top uint8, a float64) uint8 {
	return uint8(float64(base)*(1-a) + float64(top)*a + 0.5)
}

func clamp01(v float64) float64 {
	if v < 0 {
		return 0
	}
	if v > 1 {
		return 1
	}
	return v
}
