package io.github.hyeonsikoh.sidedesk.desk

import org.junit.Assert.assertEquals
import org.junit.Test

class SideLayoutTest {
    @Test fun `4대3 은 한 줄 20퍼센트`() {
        val side = sideLayout(838f, 625f)
        assertEquals(1, side.columns)
        assertEquals(838f * 0.2f, side.width, 0.01f)
    }

    @Test fun `한 줄 폭은 140에서 196 사이`() {
        assertEquals(196f, sideLayout(1200f, 900f).width, 0f)
        assertEquals(140f, sideLayout(600f, 500f).width, 0f)
    }

    @Test fun `긴 화면은 두 줄`() {
        val side = sideLayout(890f, 387f)
        assertEquals(2, side.columns)
        assertEquals(890f * 0.15f * 2 + 10f, side.width, 0.01f)
        assertEquals(2, sideLayout(1600f, 900f).columns)
        assertEquals(1, sideLayout(1500f, 900f).columns)
    }

    @Test fun `두 줄에서 홀수면 첫 칸 혼자`() {
        assertEquals(listOf(listOf(1), listOf(2, 3), listOf(4, 5)), sideRows(listOf(1, 2, 3, 4, 5), 2))
        assertEquals(listOf(listOf(1, 2), listOf(3, 4)), sideRows(listOf(1, 2, 3, 4), 2))
        assertEquals(5, sideRows(listOf(1, 2, 3, 4, 5), 1).size)
        assertEquals(emptyList<List<Int>>(), sideRows(emptyList<Int>(), 2))
    }
}

