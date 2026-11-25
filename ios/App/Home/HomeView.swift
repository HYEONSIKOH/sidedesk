import SwiftUI

struct HomeView: View {
    let model: HomeModel

    var body: some View {
        Text(model.problem ?? model.computerName ?? "컴퓨터를 기다리는 중")
            .font(.title)
            .multilineTextAlignment(.center)
            .padding(24)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .preferredColorScheme(.dark)
    }
}
