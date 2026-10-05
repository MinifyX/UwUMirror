FFmpeg for UwUMirror's sound
============================

The Windows and macOS downloads carry libavcodec and libavutil from FFmpeg
(https://ffmpeg.org), so that AirPlay's sound plays without installing
anything. They are built by scripts/build-ffmpeg.sh from the unmodified
source of FFmpeg 9.0.2:

  https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
  SHA-256 8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e

with only the AAC and ALAC decoders enabled, and nothing that is GPL or
non-free. FFmpeg is licensed under the GNU Lesser General Public License,
version 2.1 or later: https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html
You may replace these libraries with your own build of the same major
version; UwUMirror loads them at runtime.
