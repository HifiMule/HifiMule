Synthetic 16×16 black video with no audio, generated for scanner classification regression.

`ffmpeg -v error -f lavfi -i color=c=black:s=16x16:d=0.1 -an -c:v mpeg4 generated-video-only.mp4`
