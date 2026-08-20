import Foundation

enum NotchTabPack {
    static func split(
        places: [Place],
        leftCap: CGFloat,
        widthOf: (Place) -> CGFloat,
        spacing: CGFloat = 2
    ) -> (leading: [Place], trailing: [Place]) {
        guard !places.isEmpty else {
            return ([], [])
        }
        if !leftCap.isFinite {
            return (places, [])
        }

        var leading: [Place] = []
        var used: CGFloat = 0
        for (index, place) in places.enumerated() {
            let width = widthOf(place)
            let extra = leading.isEmpty ? width : spacing + width
            if leading.isEmpty || used + extra <= leftCap {
                leading.append(place)
                used += extra
            } else {
                return (leading, Array(places[index...]))
            }
        }
        return (leading, [])
    }
}
