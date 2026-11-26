package io.github.hyeonsikoh.sidedesk.link

/** 컴퓨터가 모아 보낼 수 있는 것. 시계는 폰 시각을 써서 없다. */
enum class Topic { Weather, Ai, Perf, Processes, Shortcuts, Music }

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
data class CommandResult(val commandId: Long, val ok: Boolean, val reason: String = "")

/** 컴퓨터에 보내는 명령. 번호마다 한 번만 실행되고 결과가 하나 온다. */
sealed interface Command {
    val commandId: Long
}

data class KillCommand(override val commandId: Long, val pid: Int, val startedAt: Long) : Command

/** book 은 목록을 받을 때 온 그 컴퓨터의 장부 번호다. LinkOwner 가 채운다. */
data class RunShortcut(override val commandId: Long, val shortcutId: Int, val book: Long = 0) : Command

enum class MusicPlayer { None, Music, Spotify, System }

/**
 * 지금 재생 중인 곡. player 가 None 이면 곡이 없다.
 * receivedAt 은 이 값을 받은 폰의 부팅 뒤 시각(elapsedRealtime, 밀리초)이다. 진행 바는 이것을 기준으로 폰이 스스로 움직인다.
 */
data class NowPlaying(
    val player: MusicPlayer,
    val title: String,
    val artist: String,
    val album: String,
    /** 0 은 모름(라디오, 생방송) */
    val durationMs: Long,
    /** 그림 해시. 0 은 없음 */
    val artwork: Long,
    val playing: Boolean,
    val positionMs: Long,
    val canPlay: Boolean,
    val canNext: Boolean,
    val canPrevious: Boolean,
    val receivedAt: Long = 0,
)

data class Volume(val available: Boolean, val level: Int, val muted: Boolean, val settable: Boolean)

enum class MusicAction { PlayPause, Next, Previous, SetVolume }

/** level 은 SetVolume 일 때만 쓴다. */
data class MusicCommand(override val commandId: Long, val action: MusicAction, val level: Int = 0) : Command

/** 소리 끄기, 방해 금지처럼 켜짐 상태가 있는 버튼. None 은 누르면 한 번 하는 것 */
enum class Toggle { None, On, Off }

/** 등록 내용(앱, 경로, 주소, 키)은 오지 않는다. 폰은 이름, 그림, 색만 안다. */
data class Shortcut(
    val id: Int,
    val name: String,
    val icon: String,
    val color: String,
    val toggle: Toggle,
    /** 시스템 동작(소리, 방해 금지, 잠금)이면 칸에 색 그림만 그린다. */
    val system: Boolean,
)
