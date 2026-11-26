package io.github.hyeonsikoh.sidedesk.link

/** 컴퓨터가 모아 보낼 수 있는 것. 시계는 폰 시각을 써서 없다. */
enum class Topic { Perf, Processes }

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
