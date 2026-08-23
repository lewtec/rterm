import AppKit
import SwiftUI

struct PlaceTabHandle: NSViewRepresentable {
    let id: UUID
    var onSelect: () -> Void
    var onMove: (UUID) -> Void
    var onTargeted: (Bool) -> Void
    var onReconnect: () -> Void
    var onEdit: () -> Void
    var onDelete: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> PlaceTabHandleView {
        let view = PlaceTabHandleView()
        updateNSView(view, context: context)
        return view
    }

    func updateNSView(_ view: PlaceTabHandleView, context: Context) {
        context.coordinator.onSelect = onSelect
        context.coordinator.onMove = onMove
        context.coordinator.onTargeted = onTargeted
        context.coordinator.onReconnect = onReconnect
        context.coordinator.onEdit = onEdit
        context.coordinator.onDelete = onDelete
        view.id = id
        view.coordinator = context.coordinator
    }

    final class Coordinator {
        var onSelect: () -> Void = {}
        var onMove: (UUID) -> Void = { _ in }
        var onTargeted: (Bool) -> Void = { _ in }
        var onReconnect: () -> Void = {}
        var onEdit: () -> Void = {}
        var onDelete: () -> Void = {}
    }
}

final class PlaceTabHandleView: NSControl, NSDraggingSource {
    var id = UUID()
    var coordinator: PlaceTabHandle.Coordinator?
    private var mouseDownEvent: NSEvent?
    private var mouseDownPoint: NSPoint = .zero

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        registerForDraggedTypes([.string])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override var mouseDownCanMoveWindow: Bool { false }
    override var isOpaque: Bool { false }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        true
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        bounds.contains(point) ? self : nil
    }

    override func mouseDown(with event: NSEvent) {
        mouseDownEvent = event
        mouseDownPoint = convert(event.locationInWindow, from: nil)
    }

    override func mouseDragged(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let distance = hypot(point.x - mouseDownPoint.x, point.y - mouseDownPoint.y)
        guard distance >= 4, let mouseDownEvent else {
            return
        }
        let item = NSPasteboardItem()
        item.setString(id.uuidString, forType: .string)
        let dragging = NSDraggingItem(pasteboardWriter: item)
        dragging.setDraggingFrame(bounds, contents: nil)
        beginDraggingSession(with: [dragging], event: mouseDownEvent, source: self)
        self.mouseDownEvent = nil
    }

    override func mouseUp(with event: NSEvent) {
        if mouseDownEvent != nil {
            coordinator?.onSelect()
        }
        mouseDownEvent = nil
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        let menu = NSMenu()
        menu.addItem(withTitle: "Reconnect", action: #selector(reconnect), keyEquivalent: "")
        menu.addItem(withTitle: "Edit…", action: #selector(editPlace), keyEquivalent: "")
        menu.addItem(withTitle: "Delete", action: #selector(deletePlace), keyEquivalent: "")
        for item in menu.items {
            item.target = self
        }
        return menu
    }

    func draggingSession(
        _ session: NSDraggingSession,
        sourceOperationMaskFor context: NSDraggingContext
    ) -> NSDragOperation {
        .move
    }

    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        coordinator?.onTargeted(true)
        return .move
    }

    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
        .move
    }

    override func draggingExited(_ sender: NSDraggingInfo?) {
        coordinator?.onTargeted(false)
    }

    override func prepareForDragOperation(_ sender: NSDraggingInfo) -> Bool {
        sourceID(from: sender) != nil
    }

    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        coordinator?.onTargeted(false)
        guard let sourceID = sourceID(from: sender) else {
            return false
        }
        coordinator?.onMove(sourceID)
        return true
    }

    override func draggingEnded(_ sender: NSDraggingInfo) {
        coordinator?.onTargeted(false)
    }

    @objc private func reconnect() {
        coordinator?.onReconnect()
    }

    @objc private func editPlace() {
        coordinator?.onEdit()
    }

    @objc private func deletePlace() {
        coordinator?.onDelete()
    }

    private func sourceID(from sender: NSDraggingInfo) -> UUID? {
        sender.draggingPasteboard.string(forType: .string).flatMap(UUID.init(uuidString:))
    }
}
