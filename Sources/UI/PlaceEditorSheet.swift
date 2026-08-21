import SwiftUI

struct PlaceEditorSheet: View {
    @Environment(AppStore.self) private var store

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(store.editor?.originID == nil ? "Add Place" : "Edit Place")
                .font(.title2)
            Form {
                TextField("User", text: user, prompt: Text(Place.isLoopbackHost(store.editor?.host ?? "") ? NSUserName() : "required if remote"))
                TextField("Host", text: host, prompt: Text("empty = localhost"))
                Picker("Backend", selection: backend) {
                    ForEach(Backend.allCases) { value in
                        Text(value.rawValue).tag(value)
                    }
                }
                if store.editor?.backend != .herdr {
                    TextField("Session", text: session)
                }
                TextField("Label", text: label, prompt: Text("optional"))
                Text(CatalogPaths.placesFile.path)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
            .formStyle(.grouped)
            HStack {
                Spacer()
                Button("Cancel") {
                    store.editor = nil
                }
                .keyboardShortcut(.cancelAction)
                Button("Save") {
                    store.saveEditor()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(!canSave)
            }
        }
        .padding(20)
        .frame(width: 420)
    }

    private var canSave: Bool {
        guard let editor = store.editor else { return false }
        let user = editor.user.trimmingCharacters(in: .whitespacesAndNewlines)
        let host = editor.host.trimmingCharacters(in: .whitespacesAndNewlines)
        if !Place.isLoopbackHost(host) && user.isEmpty {
            return false
        }
        if editor.backend != .herdr
            && editor.session.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        {
            return false
        }
        return true
    }

    private var user: Binding<String> {
        binding(\.user)
    }

    private var host: Binding<String> {
        binding(\.host)
    }

    private var session: Binding<String> {
        binding(\.session)
    }

    private var label: Binding<String> {
        binding(\.label)
    }

    private var backend: Binding<Backend> {
        Binding(
            get: { store.editor?.backend ?? .herdr },
            set: { store.editor?.backend = $0 }
        )
    }

    private func binding(_ keyPath: WritableKeyPath<PlaceEditorState, String>) -> Binding<String> {
        Binding(
            get: { store.editor?[keyPath: keyPath] ?? "" },
            set: { store.editor?[keyPath: keyPath] = $0 }
        )
    }
}
