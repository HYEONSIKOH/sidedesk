package io.github.hyeonsikoh.sidedesk.music

import io.github.hyeonsikoh.sidedesk.ai.Buzzer
import io.github.hyeonsikoh.sidedesk.link.AlertKind
import io.github.hyeonsikoh.sidedesk.link.CommandResult
import io.github.hyeonsikoh.sidedesk.link.FakeLink
import io.github.hyeonsikoh.sidedesk.link.LinkOwner
import io.github.hyeonsikoh.sidedesk.link.LinkState
import io.github.hyeonsikoh.sidedesk.link.MusicAction
import io.github.hyeonsikoh.sidedesk.link.MusicCommand
import io.github.hyeonsikoh.sidedesk.link.MusicPlayer
import io.github.hyeonsikoh.sidedesk.link.NowPlaying
import io.github.hyeonsikoh.sidedesk.link.Volume
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class MusicViewModelTest {
    private val main = UnconfinedTestDispatcher()
    private val link = FakeLink()
    private var clock = 1_000_000L
    private var next = 100L
    private var taps = 0
    private val buzzer = object : Buzzer {
        override fun buzz(kind: AlertKind) = Unit

        override fun tap() {
            taps += 1
        }
    }
    private val song = NowPlaying(MusicPlayer.Music, "여름 바다", "가수", "앨범", 221_000, 0, false, 84_000, true, true, true, receivedAt = 1_000_000)

    @Before
    fun setUp() {
        Dispatchers.setMain(main)
        link.state.value = LinkState.Connected("맥")
        link.nowPlaying.value = song
        link.volume.value = Volume(true, 40, false, true)
    }

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun vm() = MusicViewModel(link, buzzer, { next++ }) { clock }

    @Test
    fun movesTheProgressFromWhenItArrived() {
        val playing = song.copy(playing = true)
        assertEquals(84_000, positionAt(playing, 1_000_000))
        assertEquals(87_500, positionAt(playing, 1_003_500))
        assertEquals(84_000, positionAt(song, 1_003_500))
        assertEquals(221_000, positionAt(playing, 9_000_000))
        assertEquals(1_084_000, positionAt(playing.copy(durationMs = 0), 2_000_000))
        assertEquals(86_000, positionAt(playing, 1_010_000, lostAt = 1_002_000))
    }

    @Test
    fun writesClockText() {
        assertEquals("1:24", clockText(84_000))
        assertEquals("1:02:03", clockText(3_723_000))
        assertEquals("0:00", clockText(-5))
    }

    @Test
    fun showsPlayingFirstThenFollowsTheComputer() {
        val vm = vm()
        vm.playPause()
        assertEquals(1, taps)
        assertTrue(vm.state.value.playing)
        assertEquals(listOf(MusicCommand(100, MusicAction.PlayPause)), link.kills)
        link.nowPlaying.value = song.copy(playing = false, positionMs = 85_000)
        assertFalse(vm.state.value.playing)
    }

    @Test
    fun revertsWhenPlayPauseFails() {
        val vm = vm()
        vm.playPause()
        link.results.tryEmit(CommandResult(100, false, "음악 앱 제어를 허용해 주세요. 설정 창의 안내를 보세요"))
        assertFalse(vm.state.value.playing)
        assertEquals("음악 앱 제어를 허용해 주세요. 설정 창의 안내를 보세요", vm.state.value.notice)
    }

    @Test
    fun revertsWhenNoNewStateComesAfterSuccess() {
        val vm = vm()
        vm.playPause()
        link.results.tryEmit(CommandResult(100, true, ""))
        assertTrue(vm.state.value.playing)
        main.scheduler.advanceTimeBy(MusicViewModel.SETTLE_MILLIS + 1)
        assertFalse(vm.state.value.playing)
        assertNull(vm.state.value.notice)
    }

    @Test
    fun sendsNextAndPrevious() {
        val vm = vm()
        vm.next()
        vm.previous()
        assertEquals(listOf(MusicCommand(100, MusicAction.Next), MusicCommand(101, MusicAction.Previous)), link.kills)
    }

    @Test
    fun blocksButtonsThePlayerDisallows() {
        link.nowPlaying.value = song.copy(canNext = false, canPlay = false)
        val vm = vm()
        vm.next()
        vm.playPause()
        vm.previous()
        assertEquals(listOf(MusicCommand(100, MusicAction.Previous)), link.kills)
    }

    @Test
    fun sendsNothingWithoutASongOrWhileLost() {
        link.nowPlaying.value = song.copy(player = MusicPlayer.None)
        val vm = vm()
        vm.playPause()
        link.nowPlaying.value = song
        link.state.value = LinkState.Lost("맥")
        vm.next()
        assertTrue(link.kills.isEmpty())
        assertEquals(0, taps)
    }

    @Test
    fun saysNotConnectedWhenTheLinkRefuses() {
        link.acceptMusic = false
        val vm = vm()
        vm.next()
        assertEquals(LinkOwner.NOT_CONNECTED, vm.state.value.notice)
    }

    @Test
    fun stopsTheProgressWhenLost() {
        val vm = vm()
        clock = 1_005_000
        link.state.value = LinkState.Lost("맥")
        assertEquals(1_005_000L, vm.state.value.lostAt)
        link.state.value = LinkState.Connected("맥")
        assertNull(vm.state.value.lostAt)
    }

    @Test
    fun sendsTheVolumeAtMostEveryTenthOfASecond() {
        val vm = vm()
        vm.dragVolume(50)
        clock += 30
        vm.dragVolume(55)
        vm.dragVolume(60)
        assertEquals(listOf(MusicCommand(100, MusicAction.SetVolume, 50)), link.kills)
        assertEquals(60, vm.state.value.level)
        main.scheduler.advanceTimeBy(MusicViewModel.VOLUME_GAP_MILLIS)
        assertEquals(MusicCommand(101, MusicAction.SetVolume, 60), link.kills.last())
        vm.releaseVolume()
        assertEquals(MusicCommand(102, MusicAction.SetVolume, 60), link.kills.last())
        assertNull(vm.state.value.dragging)
    }

    @Test
    fun ignoresTheComputerVolumeWhileDragging() {
        val vm = vm()
        vm.dragVolume(70)
        link.volume.value = Volume(true, 20, false, true)
        assertEquals(70, vm.state.value.level)
        vm.releaseVolume()
        assertEquals(20, vm.state.value.level)
    }

    @Test
    fun doesNotSendToAFixedVolume() {
        link.volume.value = Volume(true, 40, false, false)
        val vm = vm()
        vm.dragVolume(70)
        assertTrue(link.kills.isEmpty())
        assertNull(vm.state.value.dragging)
    }

    @Test
    fun showsAVolumeFailureOncePerDrag() {
        val vm = vm()
        vm.dragVolume(50)
        clock += 200
        vm.dragVolume(60)
        link.results.tryEmit(CommandResult(100, false, "볼륨을 바꿀 수 없는 장치입니다"))
        assertEquals("볼륨을 바꿀 수 없는 장치입니다", vm.state.value.notice)
        main.scheduler.advanceTimeBy(MusicViewModel.NOTICE_MILLIS + 1)
        link.results.tryEmit(CommandResult(101, false, "볼륨을 바꿀 수 없는 장치입니다"))
        assertNull(vm.state.value.notice)
    }

    @Test
    fun picksTheArtworkOfTheSong() {
        link.artworks.value = mapOf(5L to byteArrayOf(1, 2))
        link.nowPlaying.value = song.copy(artwork = 5)
        val vm = vm()
        assertTrue(vm.state.value.art!!.contentEquals(byteArrayOf(1, 2)))
        link.nowPlaying.value = song.copy(artwork = 6)
        assertNull(vm.state.value.art)
    }
}
