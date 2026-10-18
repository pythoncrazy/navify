import json, random, sys, time, urllib.parse, urllib.request

B = (sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:4535") + "/rest/"
R = random.Random(3)


def call(m, **q):
    q = {"u": "admin", "p": "adminpw", "v": "1.16.1", "c": "seed", "f": "json", **q}
    url = B + m + "?" + urllib.parse.urlencode(q, doseq=True)
    return json.load(urllib.request.urlopen(url))["subsonic-response"]


songs = call("search3", query="", songCount=500, albumCount=0, artistCount=0)["searchResult3"]["song"]
albums = {}
for s in songs:
    albums.setdefault(s["album"], []).append(s)
ids = [s["id"] for s in songs]
for name in ["Focus", "Late Night Drive", "Morning Coffee", "Rainy Days", "Road Trip", "Sunday Reset"]:
    call("createPlaylist", name=name, songId=R.sample(ids, 14))
call("star", id=R.sample(ids, 23))
for name in ["Frequencies", "Wild Weather", "Salt and Signal", "Glow Season", "Long Weekend"]:
    for s in albums[name]:
        call("scrobble", id=s["id"], submission="true", time=int(time.time() * 1000))
        time.sleep(0.05)
for s in R.sample(songs, 30):
    for _ in range(R.randrange(1, 6)):
        call("scrobble", id=s["id"], submission="true")
q = sorted(albums["Sunset Strip"], key=lambda s: s["track"])
call("savePlayQueue", id=[s["id"] for s in q], current=q[0]["id"], position=0)
print(len(songs), "songs", len(albums), "albums")
