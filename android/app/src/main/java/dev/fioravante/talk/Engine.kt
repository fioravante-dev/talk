package dev.fioravante.talk

/** O motor local: o whisper.cpp na CPU do celular (android/rust). Um modelo por vez. */
object Engine {
    init {
        System.loadLibrary("talk_android")
    }

    /** Carrega (ou mantém, se já é o mesmo). Lança IllegalStateException se não der. */
    @JvmStatic external fun load(path: String): Boolean

    /** JSON: text, language, stt_seconds — ou error. Áudio mono, 16 kHz. */
    @JvmStatic external fun transcribe(
        audio: FloatArray,
        language: String,
        vocabTxt: String,
        vocab: String,
        vad: String,
    ): String

    @JvmStatic external fun unload()

    /** As trocas do vocabulário (vocab.txt do PC), para o texto do Parakeet. */
    @JvmStatic external fun applyVocab(text: String, vocabTxt: String, vocab: String): String
}
