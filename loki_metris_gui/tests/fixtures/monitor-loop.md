测试资源由本项目按 GIF89a / PNG APNG 标准结构生成，不含用户文件。monitor-loop.gif 为黑白两帧无限循环，每帧 200ms；monitor-finite.png 为黑白两帧 APNG，总播放两次，每帧 200ms。

`monitor-interlaced.png` 同样由 Python 标准库 struct/zlib 生成，3×3 Adam7 APNG，第二帧是偏移 (1,1) 的 2×2 蓝色子帧，用于回归全画布与子帧行跨度。
