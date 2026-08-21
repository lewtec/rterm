import Foundation

/// Restarts on `pet()`. Fires `onFire` after `interval` with no pet.
final class Watchdog: NSObject {
    private let interval: TimeInterval
    private let onFire: () -> Void
    private var timer: Timer?

    init(interval: TimeInterval, onFire: @escaping () -> Void) {
        self.interval = interval
        self.onFire = onFire
    }

    func pet() {
        timer?.invalidate()
        let timer = Timer(timeInterval: interval, target: self, selector: #selector(fire), userInfo: nil, repeats: false)
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    func cancel() {
        timer?.invalidate()
        timer = nil
    }

    @objc private func fire() {
        onFire()
    }

    deinit {
        timer?.invalidate()
    }
}
