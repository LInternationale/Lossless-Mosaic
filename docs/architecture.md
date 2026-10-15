# 领域、编辑模型与文件协议

## 边界

核心以 Bitmap、Rect、Mode、GridSelection 建模，公开 inspect、protect_grid、restore、transform_grid、suggest_grid 用例，保留 protect / transform_blocks 兼容 API。输入为借用字节，输出为新字节，无文件系统、Tauri 或网络依赖，失败不修改调用方原始数据。

领域模块不依赖其他项目模块；BMP、密码、载体适配器仅在 crate 内可见，用例组合它们。桌面层管理原生文件对话框、单文档会话、后台任务和另存为，前端只提交蒙版与参数，不能任意读写字符串路径。

保存使用 create_new，失败不覆盖既有文件，附加文件写入失败会清理本次新建 BMP。双文件保存不是文件系统事务，断电可能只留下其中一个文件。

## 网格与笔刷

GridSelection 是协议层位图。block_size=1 表示 α / 附加文件模式的逐像素蒙版；8/16/32/64 表示区块模式的网格，锚定左上角，边缘按实际尺寸截断。bits 逐行存储，每字节低位优先，末尾未使用位为零。零附加数据变换与熵检测拒绝 1 像素模式。

像素模式以像素中心到连续线段的距离绘制圆形笔刷扫掠区域，1 px 笔刷选择轨迹经过的像素。区块模式使用线段与扩张网格矩形求交，快速拖动不漏格。打码赋值为选中，反打码赋值为未选中，重复涂抹幂等。每个完整笔触保留一次快照，最多撤销 50 次，支持重做与 Esc 回滚。

ImageViewport 管理缩放与平移。缩放保持指针下的原图点不变，输入事件经逆变换映射原图坐标。原分辨率 PNG 通过 data URL 传递，避免把 RGBA 作为数千万个 JSON 数字传输；界面统一显示不透明 RGB。笔刷移动时只重绘轨迹附近的蒙版。

## 覆盖编辑

认证模式先用原密码完整还原，再按最终目标蒙版重新生成载体，避免重叠区域重复 XOR。密码或完整性验证失败时保留旧文档和前端未应用蒙版。删除全部网格并应用时输出源文件，不生成空载体。

区块模式在当前会话持有源字节与已应用蒙版，以源文件生成最终目标结果，重新划分尺寸也不会与旧密文叠加。再次打开零附加数据文件时无法知道旧蒙版，用户可以选择全部已有加密网格并标记为已有打码。后端先按用户标记和原密码恢复源字节，再应用目标蒙版；此流程无法检测密码或标记是否正确。会话源字节只在内存中保存。

## 还原契约

- 附加文件：选中网格的 BGR(A) 加密，其余字节保持不变；恢复完整文件 SHA-256 必须一致。
- α / 24 位：创建 top-down 32 位载体，新增 α 存储还原信息及原图非 BGR 骨架。最终重建原头、方向、填充、尾部和 BGR，逐字节验证。
- α / 32 位：丢弃原 α，保留 BGR 和文件结构，恢复后 α 全部为 255；原图哈希基于此归一化结果。UI 和后端都要求确认透明度丢失。
- 零附加区块：不存元数据、盐、nonce、标签或哈希。每格 BGR(A) 与确定性密钥流 XOR；正确密码、尺寸、位深、网格大小及位置再次变换恢复所有字节。错误密码或误判块无法检测。

## 载体协议

v1 和 v2 使用相同外层容器与 JSON manifest，拒绝未知字段，固定 KDF 参数，不从文件接受可放大计算量的 KDF 参数。

```text
Manifest {
  version: 1 | 2,
  mode: alpha | sidecar,
  salt: 16 bytes,
  original_len,
  original_hash: SHA256,
  protected_hash: SHA256,
  skeleton: bytes (仅 α / 24 位),
  records: [{ rect: Rect | null, nonce: 24 bytes, tag: 16 bytes }],
  auth_nonce: 24 bytes,
  auth_tag: 16 bytes,
  selection: { block_size, bits } (仅 v2)
}
```

v1 维持原有矩形记录和最多 256 个不重叠区域的限制，selection 不序列化，保持旧 JSON 认证兼容。其 AAD 为 `lossless-mosaic/v1/{RustModeName}/{x},{y},{width},{height}`。继续编辑时保留矩形的准确像素边界。

v2 records 恰好一个，rect 为 null。区块按网格索引升序拼接 BGR(A)。像素蒙版先逐行合并连续像素段，再纵向合并相邻行起点和宽度相同的段，按矩形首次出现顺序拼接，避免给每个像素单独分配缓冲。统一使用随机 nonce 与 tag，AAD 为 `lossless-mosaic/grid/v2/{RustModeName}`。selection 受 manifest 认证保护，解密与加密共享相同遍历规则。

元数据认证时将 auth_tag 置零，规范序列化整个 JSON 为 AAD，用独立随机 auth_nonce 对空消息执行 AEAD。Alpha / Sidecar / Entropy 枚举格式名称属于协议，不可无版本变更地修改。

- 附加文件：JSON + JSON 长度（u64 LE）+ LMMASK01（8 bytes）。
- α：按逻辑行序提取 α，存为 LMMASK01 + JSON 长度（u64 LE）+ JSON，剩余全零。protected_hash 在所有 α 清零时计算，解码提取 JSON 后同样全清零再验证。
- 零附加区块：无额外字段。盐为 `SHA256("lossless-mosaic/blocks/v1/{width}/{height}/{bpp}")[0..16]`，每格 nonce 为 `SHA256(AAD(Entropy, rect))[0..24]`，每格从密钥流偏移零开始。原版使用 32 像素；新实现允许四种尺寸，nonce 的矩形尺寸与网格对应。同密码同几何参数复用密钥流，安全性限制见 README。

## 资源与失败策略

BMP 限制 64 MiB、1600 万像素、BI_RGB、24/32 位、40/108/124 字节 DIB。检查宽高、单 plane、像素偏移、行步幅、实际长度和溢出。保留 bfSize 原值与尾部，尽量逐字节往返，不执行通用图像重编码。

manifest 输入最多 16 MiB。v2 不再受 256 选区限制。随机数失败直接报错；α 容量不足拒绝，不截断。认证密钥及命令密码缓冲使用 zeroize。

熵对网格 BGR 计算 Shannon entropy，阈值为 `min(7.6, log2(样本量) × 0.94)`，对小格修正有限样本上限。高频原图可能误报，小边缘格可能漏报，建议结果必须可用笔刷修正。无附加数据模式无法证明检测结果或解码正确性。

## 后续

大图分块内存处理、取消 KDF、跨平台实机验证与独立安全审计。新增压缩 BMP、BI_BITFIELDS 或其他格式前，应先定义各自的逐字节还原契约。
