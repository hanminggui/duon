# Duon 安全策略与文件生命周期规范 (v1)

> 目的：抵御 SSRF（服务器端请求伪造）、资源耗尽攻击（Zip-bomb/大文件），并贯彻“临时上传文件用完即删，零磁盘常驻”的隐私与资源原则。

---

## 1. URL Fetcher 防 SSRF 规范

所有传入的 `document.url` 在发起抓取前必须通过严格的安全过滤：

### 1.1 协议白名单
- 仅允许 `http` 与 `https` 协议。
- 严禁 `file://`, `gopher://`, `ftp://`, `ldap://`, `dict://` 等所有非 HTTP 协议。

### 1.2 DNS 预解析与 IP 黑名单校验
在建立 TCP 连接前，必须先进行 DNS 解析，且解析出的目标 IP 不得落入以下受保护网段：

| 类别 | IPv4 禁止网段 | IPv6 禁止网段 |
| :--- | :--- | :--- |
| **环回地址 (Loopback)** | `127.0.0.0/8` | `::1/128` |
| **私有网段 (RFC 1918)** | `10.0.0.0/8`<br>`172.16.0.0/12`<br>`192.168.0.0/16` | `fc00::/7` (Unique Local) |
| **链路本地 (Link-Local / 云元数据)** | `169.254.0.0/16` (阻断 AWS/GCP 169.254.169.254) | `fe80::/10` |
| **组播与广播 (Multicast & Broadcast)** | `224.0.0.0/4`<br>`240.0.0.0/4`<br>`255.255.255.255/32` | `ff00::/8` |
| **当前网络 (Current Network)** | `0.0.0.0/8` | `::/128` |

> **企业内网场景支持 (Intranet Support)**：
> 在企业专有云或内部微服务架构中，文件服务可能部署在内部网络（如私有 MinIO、内部 S3、内部 OSS 或集群服务 `minio.storage.svc`）。
> 系统支持通过环境变量 `DUON_ALLOW_PRIVATE_IPS=true` 或指定域名/主机白名单 `DUON_SSRF_ALLOWED_HOSTS=minio.internal,storage.corp` 允许访问私有网段；
> 无论是否开启内网放行，系统**始终强制拦截**高危的云元数据网段（`169.254.169.254`）和广播/多播网段，确保安全底线。


### 1.3 防 DNS Rebinding（DNS 重绑定）与重定向控制
- **连接级绑定**：获取安全 IP 后，直接连接到已验证的 IP 地址，并在 HTTP 请求头中保持原始 `Host`。
- **重定向安全重检**：禁止自动盲目跟随重定向。若服务器响应 301/302，重定向的目标 URL 必须重新经过完整的 1.1 与 1.2 校验。
- **最大重定向跳数**：限定最多允许 3 次重定向。

### 1.4 抓取资源配额
- **连接超时**：5 秒。
- **全量下载超时**：30 秒。
- **最大响应体积**：默认上限 50 MB。使用流式下载，一旦读取字节数超过阈值立即中断连接并返回 `413 Payload Too Large`。

---

## 2. 临时文件生命周期规范 (Ephemeral File Lifecycle)

针对 `multipart/form-data` 直接上传的文件：

### 2.1 零常驻原则 (Zero Retention)
- 上传的文件流优先在内存（如缓冲在指定限额内，例如 10MB）中处理。
- 若超出内存阈值写入磁盘，必须保存在隔离的临时目录（如 `/tmp/duon-ephemeral-*`）。
- **生命周期边界**：文件仅在 `LiteParse` 解析期间有效。一旦解析完成生成 `Document IR`，原始二进制文件必须立即销毁删除。

### 2.2 RAII 自动清理守卫
- 代码实现上必须采用 Rust RAII 守卫模式（实现 `Drop` 特征）：
  ```rust
  pub struct EphemeralFile {
      path: PathBuf,
  }
  
  impl Drop for EphemeralFile {
      fn drop(&mut self) {
          let _ = std::fs::remove_file(&self.path);
      }
  }
  ```
- 无论处理正常成功、中途抛出异常、或是 HTTP 连接被客户端强制中断，析构函数均保证清理临时文件，严禁泄露磁盘空间。
