package io.github.hyeonsikoh.sidedesk.link

import android.util.Log
import uniffi.sidedesk_core.generateKeyPair
import java.io.File

class Identity(val privateKey: ByteArray, val publicKey: ByteArray)

/** 앱 전용 폴더에 이 폰의 키와 짝을 맺은 컴퓨터의 공개 키를 둔다. */
class KeyFiles(private val folder: File) {
    private val identityFile = File(folder, "identity.key")
    private val computersFile = File(folder, "computers.txt")

    @Synchronized
    fun identity(): Identity {
        val bytes = identityFile.takeIf { it.exists() }?.readBytes()
        if (bytes != null && bytes.size == KEY * 2) {
            return Identity(bytes.copyOfRange(0, KEY), bytes.copyOfRange(KEY, KEY * 2))
        }
        if (bytes != null) Log.w(TAG, "키 파일이 깨져 새로 만들고 짝 목록을 비움")
        val pair = generateKeyPair()
        write(identityFile, pair.privateKey + pair.publicKey)
        computersFile.delete()
        return Identity(pair.privateKey, pair.publicKey)
    }

    @Synchronized
    fun computers(): List<ByteArray> {
        if (!computersFile.exists()) return emptyList()
        return computersFile.readLines().mapNotNull { decode(it.trim()) }
    }

    @Synchronized
    fun trust(key: ByteArray) {
        val keys = computers()
        if (keys.any { it.contentEquals(key) }) return
        write(computersFile, (keys + key).joinToString("") { encode(it) + "\n" }.toByteArray())
    }

    private fun write(file: File, bytes: ByteArray) {
        folder.mkdirs()
        val temporary = File(folder, file.name + ".tmp")
        temporary.writeBytes(bytes)
        if (!temporary.renameTo(file)) error("파일을 바꾸지 못함: ${file.name}")
    }

    private companion object {
        const val TAG = "SideDeskKeys"
        const val KEY = 32

        fun encode(key: ByteArray) = key.joinToString("") { "%02x".format(it) }

        fun decode(text: String): ByteArray? {
            if (text.length != KEY * 2) return null
            return runCatching { ByteArray(KEY) { text.substring(it * 2, it * 2 + 2).toInt(16).toByte() } }.getOrNull()
        }
    }
}
