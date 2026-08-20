enum TabState: Hashable {
    case idle
    case attaching
    case live
    case failed(String)
}
