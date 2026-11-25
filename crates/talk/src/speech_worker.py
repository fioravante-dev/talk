"""O trabalhador da fala: o Qwen3-TTS na GPU, uma frase por vez, em streaming.

O serviço (`talk-rs serve`) sobe este processo com o Python do ambiente da
fala e conversa pelo stdin/stdout, como faz com o trabalhador do Whisper:

  entra:  uma linha JSON por frase: {"text": ..., "language": "pt"|"en", "speaker": ...}
  sai:    {"samples": N, "rate": 24000}\\n + N × f32 little-endian, quantas vezes
          for, e no fim {"samples": 0, "rate": 0, "done": true[, "error": ...]}

Fechar o stdin manda o trabalhador embora; só assim a CUDA devolve a VRAM.
O binário embute este arquivo e o escreve em ~/.cache/talk/speech/ a cada subida.
"""

import json
import os
import sys

# O stdout é o fio com o serviço: nada além do protocolo pode sair por ele. As
# bibliotecas imprimem avisos à vontade; o fd 1 passa a ser o stderr (o log).
wire = os.fdopen(os.dup(1), "wb")
os.dup2(2, 1)
sys.stdout = sys.stderr

import numpy as np  # noqa: E402
from faster_qwen3_tts import FasterQwen3TTS  # noqa: E402

LANGUAGES = {"pt": "Portuguese", "en": "English"}
# Pedaços de 4 quadros do codec (~0,33 s): o primeiro áudio sai em ~0,16 s na
# 3060 e a geração segue 2× à frente do tempo real.
CHUNK = 4


def send(header, audio=None):
    wire.write((json.dumps(header) + "\n").encode())
    if audio is not None:
        wire.write(audio.tobytes())
    wire.flush()


def main():
    model = FasterQwen3TTS.from_pretrained(sys.argv[1])
    speakers = model.model.get_supported_speakers() or []
    # Aquecer captura os grafos da CUDA: sem isso a primeira frase leva segundos.
    model.generate_custom_voice(text="Oi.", speaker=speakers[0] if speakers else "vivian", language="Portuguese")
    print(f"[speech] pronto: {sys.argv[1]}, vozes {speakers}", file=sys.stderr, flush=True)

    for line in sys.stdin:
        if not line.strip():
            continue
        end = {"samples": 0, "rate": 0, "done": True}
        try:
            say = json.loads(line)
            stream = model.generate_custom_voice_streaming(
                text=say["text"],
                speaker=say["speaker"],
                language=LANGUAGES.get(say.get("language", ""), "Auto"),
                chunk_size=CHUNK,
            )
            for audio, rate, _ in stream:
                audio = np.ascontiguousarray(np.asarray(audio, dtype="<f4").reshape(-1))
                send({"samples": int(audio.size), "rate": int(rate)}, audio)
                end["rate"] = int(rate)
        except Exception as e:  # a frase falha, o trabalhador segue
            end["error"] = f"{type(e).__name__}: {e}"
        send(end)


if __name__ == "__main__":
    main()
