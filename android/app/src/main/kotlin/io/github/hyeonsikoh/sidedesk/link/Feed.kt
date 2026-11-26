package io.github.hyeonsikoh.sidedesk.link

/** 컴퓨터가 모아 보낼 수 있는 것. 시계는 폰 시각을 써서 없다. */
enum class Topic { Weather, Ai, Perf, Processes }

enum class Sky { Unknown, Clear, PartlyCloudy, Cloudy, Fog, Drizzle, Rain, Snow, Shower, Thunderstorm }

/** 날씨를 실제로 받은 곳. Unknown 이면 출처를 숨긴다. */
enum class WeatherSource { Unknown, Kma, OpenMeteo }

/** 기온은 °C, fetchedAt 은 컴퓨터가 받은 시각(유닉스 밀리초) */
data class Weather(
    val temperature: Float,
    val sky: Sky,
    val high: Float,
    val low: Float,
    val isDay: Boolean,
    val fetchedAt: Long,
    val source: WeatherSource,
)

enum class AiTool { Unknown, ClaudeCode, Codex }

/** 세션 단계. Unknown 은 기록 모양을 몰라 정하지 못한 것이다. */
enum class AiState { Unknown, Working, Waiting, Done }

/** since 는 지금 단계가 된 때(유닉스 밀리초), open 은 프로그램이 아직 떠 있는지 */
data class AiSession(
    val id: String,
    val tool: AiTool,
    val title: String,
    val project: String,
    val state: AiState,
    val since: Long,
    val open: Boolean,
)

enum class AlertKind { Unknown, Done, Waiting }

/** 작업 중에서 끝남이나 답변 대기로 바뀐 순간. id 는 컴퓨터가 늘려 간다. */
data class AiAlert(val id: Long, val sessionId: String, val title: String, val kind: AlertKind, val at: Long)

enum class ForgeState { Unknown, Ok, Missing, SignedOut, Failed }

/** 검토 요청, 내가 연 요청, 읽지 않은 알림 수 */
data class ForgeCounts(val state: ForgeState, val reviews: Int, val mine: Int, val notices: Int)

data class Forge(val github: ForgeCounts, val gitlab: ForgeCounts)

/** 기기 위치. 반올림과 바뀜 판단은 core 가 한다. */
sealed interface LocationState {
    /** 아직 모른다. 보내지 않는다. */
    data object Unknown : LocationState

    /** 권한이 없다. 위치 없음을 보낸다. */
    data object Off : LocationState

    data class At(val latitude: Double, val longitude: Double) : LocationState
}

/** 지금 보이는 것. 큰 화면은 하나, 작은 칸은 여럿. 둘 다 비면 컴퓨터는 수집을 멈춘다. */
data class Subscription(val big: Topic? = null, val small: Set<Topic> = emptySet()) {
    companion object {
        val NONE = Subscription()
    }
}

/** 사용률은 0~100. GPU 를 못 읽는 컴퓨터는 null */
data class Perf(
    val cpu: Float,
    val gpu: Float?,
    val memory: Float,
    val disk: Float,
    val receivePerSecond: Long,
    val sendPerSecond: Long,
)

data class ProcessInfo(
    val pid: Int,
    val startedAt: Long,
    val name: String,
    val cpu: Float,
    val memoryBytes: Long,
)

/** cpu 는 코어 하나가 100 이다. */
data class Processes(val list: List<ProcessInfo>)

/** 실패면 컴퓨터가 보낸 이유 한 문장 */
data class KillResult(val commandId: Long, val ok: Boolean, val reason: String = "")

data class KillCommand(val commandId: Long, val pid: Int, val startedAt: Long)
