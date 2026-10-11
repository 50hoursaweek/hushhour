#!/usr/bin/env python3
"""Generate the bundled music tracks with Google Lyria 3 through OpenRouter.

Reads OPENROUTER_API_KEY from the environment, from a `.env` file in the
folder above the project (recommended: it cannot be committed), or from a
git-ignored `.env` in the project root. Writes assets/music/<name>.mp3, which the AppImage
packaging picks up. Each full-length song costs a few cents on OpenRouter.

    python3 packaging/gen-tracks.py            # generate all three tracks
    python3 packaging/gen-tracks.py piano      # just one
    python3 packaging/gen-tracks.py --dry-run  # show the requests, spend nothing
    python3 packaging/gen-tracks.py --format=wav --out=DIR piano   # lossless, elsewhere
"""
import base64, json, os, sys, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets", "music")
MODEL = "google/lyria-3-pro-preview"
URL = "https://openrouter.ai/api/v1/chat/completions"

TRACKS = {
    "piano": (
        "Quiet piano",
        "Instrumental only, no vocals, no lyrics. A slow, gentle solo piano piece for focused "
        "writing: soft felt-piano tone played very quietly and delicately (pianissimo), sparse "
        "notes with space between them, a calm major key, subtle room reverb, around 60 bpm, "
        "no drums, no sudden changes. Quiet, intimate recording with natural dynamics and plenty "
        "of headroom: no loudness maximising, no limiting, no compression, no distortion, no "
        "clipping, peaks well below full scale. Fading out at the end.",
    ),
    "lofi": (
        "Lofi desk",
        "Instrumental only, no vocals, no lyrics. A slow, sleepy lo-fi hip hop beat for late-night "
        "writing: warm Rhodes electric piano chords, a soft, lazy boom-bap drum loop with swing, "
        "deep round bass, a few muted guitar notes, around 62 bpm, very relaxed and steady. "
        "Made as a seamless loop: no intro, no build-up, no breakdown, no ending and no fade "
        "out, the same groove and texture from the first second to the last. Clean, smooth "
        "sound: no vinyl crackle, no distortion, no clipping, natural dynamics with headroom.",
    ),
    "cinematic": (
        "Slow horizon",
        "Instrumental only, no vocals, no lyrics. A slow, atmospheric cinematic ambient piece "
        "for writing: sustained string pads, deep soft synth drones, slow gradual swells, wide "
        "stereo reverb, no percussion, no piano, hopeful but calm and unhurried. Made as a "
        "seamless loop: no intro, no climax, no ending and no fade out, a steady evolving "
        "texture from the first second to the last. Clean, smooth sound: no distortion, no "
        "clipping, natural dynamics with headroom.",
    ),
    "guitar": (
        "Nylon strings",
        "Instrumental only, no vocals, no lyrics. A slow, tender fingerpicked nylon-string classical "
        "guitar piece for writing, with a soft cello drone underneath and a little room reverb. "
        "Around 60 bpm, gentle touch, no drums, no piano, calm and unhurried. Made as a seamless "
        "loop: no intro, no climax, no ending and no fade out, the same mood from the first "
        "second to the last. Clean, smooth sound: no distortion, no clipping, natural dynamics "
        "with headroom.",
    ),
    "jazz": (
        "Late cafe",
        "Instrumental only, no vocals, no lyrics. A slow, smoky late-night jazz trio for writing: "
        "upright bass, soft brushed drums, muted trumpet, no piano, around 65 bpm, relaxed and "
        "intimate. Made as a seamless loop: no intro, no solo climax, no ending and no fade "
        "out, the same mood from the first second to the last. Clean, smooth sound: no "
        "distortion, no clipping, natural dynamics with headroom.",
    ),
    "voices": (
        "Wordless",
        "Instrumental only, no lyrics: a slow, warm wordless choral pad, soft sustained voices "
        "humming and singing 'ah' with no words, wide reverb, no percussion, calm and timeless. "
        "Made as a seamless loop: no intro, no climax, no ending and no fade out, a steady "
        "evolving texture from the first second to the last. Clean, smooth sound: no distortion, "
        "no clipping, natural dynamics with headroom.",
    ),
    "cello": (
        "Low strings",
        "Instrumental only, no vocals, no lyrics. A slow, melodic piece for solo cello over soft "
        "sustained strings, bowed and legato, no plucking, no piano, no percussion, around 55 bpm, "
        "calm, warm and gently hopeful. Made as a seamless loop: no intro, no climax, no ending "
        "and no fade out, the same mood from the first second to the last. Clean, smooth sound: "
        "no distortion, no clipping, natural dynamics with headroom.",
    ),
}


def api_key():
    key = os.environ.get("OPENROUTER_API_KEY")
    if key:
        return key
    # The parent folder (outside the repository) is checked first: a key kept
    # there can never be committed by accident.
    for env in (os.path.join(os.path.dirname(ROOT), ".env"), os.path.join(ROOT, ".env")):
        if os.path.exists(env):
            for line in open(env):
                line = line.strip()
                if line.startswith("OPENROUTER_API_KEY="):
                    return line.split("=", 1)[1].strip().strip('"').strip("'")
    sys.exit("No OPENROUTER_API_KEY in the environment, ../.env or .env")


def generate(name, title, prompt, key, fmt, out_dir, model=MODEL):
    body = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "modalities": ["text", "audio"],
        "audio": {"format": fmt},
        "stream": True,
    }
    req = urllib.request.Request(
        URL,
        data=json.dumps(body).encode(),
        headers={
            "Authorization": f"Bearer {key}",
            "Content-Type": "application/json",
            "HTTP-Referer": "https://github.com/50hoursaweek/inkpad",
            "X-Title": "HushHour",
        },
    )
    chunks, text, usage = [], [], None
    try:
        with urllib.request.urlopen(req, timeout=600) as resp:
            for raw in resp:
                line = raw.decode("utf-8", "replace").strip()
                if not line.startswith("data:"):
                    continue
                payload = line[5:].strip()
                if payload == "[DONE]":
                    break
                event = json.loads(payload)
                if "error" in event:
                    sys.exit(f"{name}: API error: {json.dumps(event['error'])}")
                usage = event.get("usage") or usage
                for choice in event.get("choices", []):
                    delta = choice.get("delta") or choice.get("message") or {}
                    audio = delta.get("audio") or {}
                    if audio.get("data"):
                        chunks.append(audio["data"])
                    if delta.get("content"):
                        text.append(delta["content"])
    except urllib.error.HTTPError as e:
        sys.exit(f"{name}: HTTP {e.code}: {e.read().decode('utf-8', 'replace')[:2000]}")
    if not chunks:
        sys.exit(f"{name}: no audio in the response. Text was: {''.join(text)[:500]!r}")
    data = base64.b64decode("".join(chunks))
    os.makedirs(out_dir, exist_ok=True)
    path = os.path.join(out_dir, f"{title}.{fmt}")
    with open(path, "wb") as f:
        f.write(data)
    cost = (usage or {}).get("cost")
    print(f"{name}: wrote {path} ({len(data) // 1024} KiB){'' if cost is None else f', cost ${cost}'}")


def main(argv):
    dry = "--dry-run" in argv
    fmt = next((a.split("=", 1)[1] for a in argv if a.startswith("--format=")), "mp3")
    out_dir = next((a.split("=", 1)[1] for a in argv if a.startswith("--out=")), OUT)
    model = next((a.split("=", 1)[1] for a in argv if a.startswith("--model=")), MODEL)
    suffix = next((a.split("=", 1)[1] for a in argv if a.startswith("--suffix=")), "")
    tag = next((a.split("=", 1)[1] for a in argv if a.startswith("--tag=")), "")
    names = [a for a in argv if not a.startswith("--")] or list(TRACKS)
    unknown = [n for n in names if n not in TRACKS]
    if unknown:
        sys.exit(f"unknown track(s): {unknown}; choose from {list(TRACKS)}")
    if dry:
        for n in names:
            print(f"{n}: {MODEL} -> {out_dir}/{TRACKS[n][0]}.{fmt}\n  {TRACKS[n][1]}\n")
        return
    key = api_key()
    for n in names:
        title, prompt = TRACKS[n]
        if tag:
            title = f"{title} ({tag})"
        generate(n, title, prompt + " " + suffix if suffix else prompt, key, fmt, out_dir, model)


if __name__ == "__main__":
    main(sys.argv[1:])
