# BaoClaw Windows Service (占位文档)

Windows Service 支持暂未实现。当前 Windows 用户请手动启动 daemon：

```powershell
# 手动启动 daemon（前台运行）
cd %USERPROFILE%\BaoClaw
.\target\release\baoclaw-core.exe --daemon

# 或用 PowerShell 后台任务
Start-Job -ScriptBlock { 
    Set-Location $env:USERPROFILE\BaoClaw
    .\target\release\baoclaw-core.exe --daemon 
}
```

## 未来计划

Windows Service 支持计划用 `windows-service` crate 实现：
- 注册为 Windows Service（sc.exe create）
- 开机自启
- 固定 socket 路径：%TEMP%/baoclaw-sockets/baoclaw.sock

欢迎贡献。
