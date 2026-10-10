#!/usr/bin/env python3
"""Generate the bundled music tracks with Google Lyria 3 through OpenRouter.

Reads OPENROUTER_API_KEY from the environment or from a `.env` file in the
project root (git-ignored). Writes assets/music/<name>.mp3, which the AppImage
packaging picks up. Each full-length song costs a few cents on OpenRouter.

    python3 packaging/gen-tracks.py            # generate all three tracks
    python3 packaging/gen-tracks.py piano      # just one
    python3 packaging/gen-tracks.py --dry-run  # show the requests, spend nothing
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
        "writing: soft felt-piano tone, sparse notes with space between them, a calm major key, "
        "subtle room reverb, around 60 bpm, no drums, no sudden changes, fading out at the end.",
    ),
    "lofi": (
        "Lofi desk",
        "Instrumental only, no vocals, no lyrics. A mellow lo-fi hip hop beat for studying: "
        "dusty vinyl crackle, warm Rhodes electric piano chords, a soft boom-bap drum loop with "
        "swing, deep round bass, muted trumpet or guitar accents, around 75 bpm, relaxed and "
        "steady, fading out at the end.",
    ),
    "cinematic": (
        "Slow horizon",
        "Instrumental only, no vocals, no lyrics. A slow, atmospheric cinematic ambient piece: "
        "sustained string pads, a distant piano motif, deep soft drones, gradual swells, wide "
        "stereo reverb, no percussion, hopeful but calm, suitable as background for writing, "
        "fading out at the end.",
    ),
}


def api_key():
    key = os.environ.get("OPENROUTER_API_KEY")
    if key:
        return key
    env = os.path.join(ROOT, ".env")
    if os.path.exists(env):
        for line in open(env):
            line = line.strip()
            if line.startswith("OPENROUTER_API_KEY="):
                return line.split("=", 1)[1].strip().strip('"').strip("'")
    sys.exit("No OPENROUTER_API_KEY in the environment or in .env")


def generate(name, title, prompt, key):
    body = {
        "model": MODEL,
        "messages": [{"role": "user", "content": prompt}],
        "modalities": ["text", "audio"],
        "audio": {"format": "mp3"},
        "stream": True,
    }
    req = urllib.request.Request(
        URL,
        data=json.dumps(body).encode(),
        headers={
            "Authorization": f"Bearer {key}",
            "Content-Type": "application/json",
            "HTTP-Referer": "https://github.com/50hoursaweek/inkpad",
            "X-Title": "Inkpad",
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
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, f"{title}.mp3")
    with open(path, "wb") as f:
        f.write(data)
    cost = (usage or {}).get("cost")
    print(f"{name}: wrote {path} ({len(data) // 1024} KiB){'' if cost is None else f', cost ${cost}'}")


def main(argv):
    dry = "--dry-run" in argv
    names = [a for a in argv if not a.startswith("--")] or list(TRACKS)
    unknown = [n for n in names if n not in TRACKS]
    if unknown:
        sys.exit(f"unknown track(s): {unknown}; choose from {list(TRACKS)}")
    if dry:
        for n in names:
            print(f"{n}: {MODEL} -> assets/music/{TRACKS[n][0]}.mp3\n  {TRACKS[n][1]}\n")
        return
    key = api_key()
    for n in names:
        title, prompt = TRACKS[n]
        generate(n, title, prompt, key)


if __name__ == "__main__":
    main(sys.argv[1:])
