enum AttachExit: Equatable {
    case clean
    case failed(String)

    static func classify(_ code: Int32?) -> AttachExit {
        switch code {
        case 0:
            return .clean
        case nil:
            return .failed("process died")
        case let code?:
            return .failed("exited (\(code))")
        }
    }
}
