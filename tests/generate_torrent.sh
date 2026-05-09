mkdir -p /tmp/data

echo "TEST1" > /tmp/data/test1.txt
echo "TEST2" > /tmp/data/test2.txt
echo "TEST3" > /tmp/data/test3.txt
echo "TEST4" > /tmp/data/test4.txt

buildtorrent \
  --announce tracker.org/announce \
  --piecelength 2 \
  /tmp/data \
  /tmp/example.torrent
