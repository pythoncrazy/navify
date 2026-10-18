import os, random, subprocess, shutil, sys

R = random.Random(7)
L = sys.argv[1] if len(sys.argv) > 1 else "/tmp/demo/music"
FONTS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../assets/fonts")
FONT, FONTB = f"{FONTS}/Figtree-Black.ttf", f"{FONTS}/Figtree-Bold.ttf"
shutil.rmtree(L, ignore_errors=True)
os.makedirs(L)

ARTISTS = {
    "Golden Hour Club": (["Long Weekend", "Sunset Strip"], ["e2742b", "3b4fd8"]),
    "Low Tide Radio": (["Frequencies", "Harbor Lights"], ["1fa2ff", "12d8fa"]),
    "Juniper Vale": (["Wild Weather", "Pine & Ember"], ["d6246e", "f29f58"]),
    "Coastal Static": (["Salt and Signal", "Paper Boats"], ["3aa76d", "e9c46a"]),
    "Neon Orchard": (["Glow Season", "Night Fruit"], ["8e2de2", "f000ff"]),
    "The Quiet Hours": (["Morning Light", "Slow Rooms"], ["283c86", "45a247"]),
    "Velvet Circuit": (["Analog Hearts", "Midnight Bus"], ["ff416c", "ff4b2b"]),
    "North Atlas": (["Open Roads", "Polar Days"], ["0f2027", "2c94b4"]),
}
WORDS = "Blue Paper Lanterns Echo Velvet Neon Coast Golden Satellite Morning Rain Glass River Silver Static Summer Window Quiet Ocean City Fade Wild Late Dream Signal Lights Drive Garden".split()
TYPES = ["linear", "radial", "circular", "linear", "spiral"]


def cover(path, top, title, c0, c1, kind):
    t = title.replace(":", r"\:").replace("'", "")
    tp = top.upper().replace("'", "")
    sun = "lt(hypot(X-300\\,Y-250)\\,130)"
    ring = "lt(abs(hypot(X-300\\,Y-270)-150)\\,7)+lt(abs(hypot(X-300\\,Y-270)-95)\\,5)"
    shape = {
        0: "drawbox=x=0:y=400:w=600:h=200:color=black@0.2:t=fill,",
        1: f"format=yuv444p,geq=lum='if({ring}\\,min(255\\,lum(X\\,Y)+70)\\,lum(X\\,Y))':cb='cb(X\\,Y)':cr='cr(X\\,Y)',",
        2: f"format=yuv444p,geq=lum='if({sun}\\,240\\,lum(X\\,Y))':cb='if({sun}\\,128\\,cb(X\\,Y))':cr='if({sun}\\,128\\,cr(X\\,Y))',",
    }[kind % 3]
    vf = (
        f"{shape}"
        f"drawtext=fontfile={FONTB}:text='{tp}':fontcolor=white@0.9:fontsize=26:x=(w-tw)/2:y=36,"
        f"drawtext=fontfile={FONT}:text='{t}':fontcolor=white:fontsize=58:x=40:y=h-th-48"
    )
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i",
         f"gradients=s=600x600:c0=0x{c0}:c1=0x{c1}:x0=0:y0=0:x1=600:y1=600:type={TYPES[kind % len(TYPES)]}:seed={kind}",
         "-vf", vf, "-frames:v", "1", "-q:v", "3", path],
        check=True,
    )


n = 0
for artist, (albums, (c0, c1)) in ARTISTS.items():
    ad = f"{L}/{artist}"
    os.makedirs(ad)
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i",
         f"gradients=s=600x600:c0=0x{c1}:c1=0x{c0}:type=radial:seed={n}",
         "-frames:v", "1", "-q:v", "3", f"{ad}/artist.jpg"],
        check=True,
    )
    for ai, album in enumerate(albums):
        d = f"{ad}/{album}"
        os.makedirs(d)
        a, b = (c0, c1) if ai == 0 else (c1, c0)
        cover(f"{d}/cover.jpg", artist, album, a, b, n)
        n += 1
        year = 2017 + R.randrange(9)
        for t in range(1, R.randrange(6, 10)):
            title = f"{R.choice(WORDS)} {R.choice(WORDS)}" if t > 1 else album
            secs = R.randrange(150, 290)
            subprocess.run(
                ["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i",
                 f"anullsrc=r=44100:cl=stereo", "-t", str(secs),
                 "-metadata", f"artist={artist}", "-metadata", f"album_artist={artist}",
                 "-metadata", f"album={album}", "-metadata", f"track={t}",
                 "-metadata", f"title={title}", "-metadata", f"date={year}",
                 "-metadata", "genre=" + R.choice(["Indie Pop", "Electronic", "Folk", "Ambient", "Rock"]),
                 f"{d}/{t:02d} {title}.flac"],
                check=True,
            )
print("albums", n)
