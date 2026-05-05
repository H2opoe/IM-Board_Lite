"""macOS 密钥提取 — 通过 C 二进制扫描微信进程内存"""

import os
import platform
import plistlib
import shlex
import subprocess
import sys
import tempfile

from .common import (
    collect_db_files,
    cross_verify_keys,
    save_results,
    scan_memory_for_keys,
    verify_enc_key,
)

DEFAULT_BUNDLE_ID = "com.tencent.xinWeChat"


def _find_binary():
    """查找对应架构的 C 二进制。"""
    machine = platform.machine()
    if machine == "arm64":
        name = "find_all_keys_macos.arm64"
    elif machine == "x86_64":
        name = "find_all_keys_macos.x86_64"
    else:
        raise RuntimeError(f"不支持的 macOS 架构: {machine}")

    # PyInstaller 运行时：从临时解压目录查找
    if getattr(sys, 'frozen', False):
        base = sys._MEIPASS
    else:
        base = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

    bin_path = os.path.join(base, "wechat_cli", "bin", name)
    if os.path.isfile(bin_path):
        return bin_path

    # fallback: 直接在 bin/ 下
    bin_path = os.path.join(base, "bin", name)
    if os.path.isfile(bin_path):
        return bin_path

    raise RuntimeError(
        f"找不到密钥提取二进制: {bin_path}\n"
        "请确认安装包完整"
    )


def _get_original_entitlements(app_path):
    """提取 app 当前的签名 entitlements，返回 dict 或 None。"""
    try:
        result = subprocess.run(
            ["codesign", "-d", "--entitlements", ":-", app_path],
            capture_output=True,
            timeout=15,
        )
        if result.returncode == 0 and result.stdout:
            return plistlib.loads(result.stdout)
    except Exception:
        pass
    return None


def _build_entitlements_xml(app_path):
    """构建 entitlements：保留原有权限 + 添加 get-task-allow。"""
    entitlements = _get_original_entitlements(app_path)
    if entitlements is None:
        entitlements = {}

    entitlements["com.apple.security.get-task-allow"] = True

    return plistlib.dumps(entitlements, fmt=plistlib.FMT_XML)


def _read_app_info(app_path):
    info_path = os.path.join(app_path, "Contents", "Info.plist")
    try:
        with open(info_path, "rb") as f:
            return plistlib.load(f)
    except Exception:
        return {}


def _find_app_by_bundle_id(bundle_id):
    if not bundle_id:
        return None

    search_roots = [
        "/Applications",
        os.path.expanduser("~/Applications"),
    ]
    for root in search_roots:
        if not os.path.isdir(root):
            continue
        try:
            names = os.listdir(root)
        except OSError:
            continue
        for name in names:
            if not name.endswith(".app"):
                continue
            app_path = os.path.join(root, name)
            info = _read_app_info(app_path)
            if info.get("CFBundleIdentifier") == bundle_id:
                return app_path
    return None


def _resolve_app_path(app_path=None, bundle_id=None):
    if app_path:
        app_path = os.path.abspath(os.path.expanduser(app_path))
        if not os.path.isdir(app_path):
            raise RuntimeError(f"微信应用不存在: {app_path}")
        return app_path

    if bundle_id:
        found = _find_app_by_bundle_id(bundle_id)
        if found:
            return found

    if not bundle_id or bundle_id == DEFAULT_BUNDLE_ID:
        for p in (
            "/Applications/WeChat.app",
            os.path.expanduser("~/Applications/WeChat.app"),
        ):
            if os.path.isdir(p):
                return p

    return None


def _app_executable_name(app_path):
    if not app_path:
        return "WeChat"
    info = _read_app_info(app_path)
    return info.get("CFBundleExecutable") or "WeChat"


def _running_pids(process_name):
    try:
        result = subprocess.run(
            ["pgrep", "-x", process_name],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except Exception:
        return []
    if result.returncode not in (0, 1):
        return []
    pids = []
    for line in result.stdout.splitlines():
        line = line.strip()
        if line.isdigit():
            pids.append(int(line))
    return pids


def _process_field(pid, field):
    try:
        result = subprocess.run(
            ["ps", "-p", str(pid), "-o", field],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except Exception:
        return ""
    if result.returncode != 0:
        return ""
    return result.stdout.strip()


def _process_command(pid):
    return _process_field(pid, "command=") or _process_field(pid, "comm=")


def _select_pid(pid=None, app_path=None, strict=False):
    if pid:
        return pid
    if not app_path:
        return None

    process_name = _app_executable_name(app_path)
    pids = _running_pids(process_name)
    if not pids:
        raise RuntimeError(f"未找到正在运行的微信进程: {process_name}")

    app_real = os.path.realpath(app_path)
    matches = []
    details = []
    for candidate in pids:
        command = _process_command(candidate)
        details.append(f"{candidate}: {command or '(unknown)'}")
        command_real = os.path.realpath(command) if command else ""
        if (
            command.startswith(app_path + os.sep)
            or command_real.startswith(app_real + os.sep)
            or f"{app_path}/Contents/MacOS/" in command
            or f"{app_real}/Contents/MacOS/" in command_real
        ):
            matches.append(candidate)

    if len(matches) == 1:
        return matches[0]
    if len(matches) > 1:
        raise RuntimeError(
            "匹配到多个微信进程，请通过 --pid 指定其中一个:\n  "
            + "\n  ".join(details)
        )
    if len(pids) == 1 and not strict:
        print("[!] 未能通过应用路径确认进程，将使用唯一的 WeChat 进程。")
        print(f"    PID: {pids[0]}")
        print(f"    进程: {details[0].split(': ', 1)[1]}")
        return pids[0]

    raise RuntimeError(
        f"未找到来自 {app_path} 的微信进程，请确认该应用已启动，或通过 --pid 指定。\n"
        "当前 WeChat 进程:\n  "
        + "\n  ".join(details)
    )


def _resign_wechat(app_path=None, bundle_id=None):
    """Re-sign WeChat: 保留原有 entitlements，仅添加 get-task-allow。"""
    wechat_app = _resolve_app_path(app_path, bundle_id)
    if wechat_app is None:
        target = f"Bundle ID {bundle_id}" if bundle_id else "WeChat.app"
        return False, f"未找到 {target}（已搜索 /Applications 和 ~/Applications）"

    print(f"\n[*] 检测到 task_for_pid 权限不足，正在对微信重新签名...")
    print(f"    目标: {wechat_app}")

    # 提取并合并 entitlements
    try:
        ent_data = _build_entitlements_xml(wechat_app)
    except Exception as e:
        return False, f"提取微信原始权限失败: {e}"

    ent_fd, ent_path = tempfile.mkstemp(suffix=".plist")
    try:
        with os.fdopen(ent_fd, "wb") as f:
            f.write(ent_data)

        result = subprocess.run(
            ["codesign", "--force", "--sign", "-", "--entitlements", ent_path, wechat_app],
            capture_output=True,
            text=True,
            timeout=60,
        )
    finally:
        os.unlink(ent_path)

    if result.returncode != 0:
        stderr = result.stderr.strip()
        if "Operation not permitted" in stderr:
            return False, (
                "codesign 被 macOS 的 App 管理权限拦截（Operation not permitted）。"
                "请在系统提示“已阻止修改 Mac 上的 App”时选择允许；"
                "如果刚才点了不允许，请到 系统设置 > 隐私与安全性 > App 管理 允许当前应用或终端。"
            )
        return False, f"codesign 失败: {stderr}"

    print("[+] 签名完成（已保留微信原有权限，仅添加调试访问权限）。")
    print("[+] 请重新启动微信后再执行 init。")
    print("[!] 注意：如果微信自动更新，可能需要重新签名。")
    return True, None


def extract_keys(db_dir, output_path, pid=None, app_path=None, bundle_id=None, container_id=None):
    """通过 C 二进制提取 macOS 微信数据库密钥。

    C 二进制在微信数据目录的父目录下运行，并只扫描传入的 db_storage。
    输出 all_keys.json 到当前工作目录。

    Args:
        db_dir: 微信 db_storage 目录
        output_path: all_keys.json 输出路径
        pid: 可选，指定微信进程 PID
        app_path: 可选，指定微信 .app 路径，用于多开进程匹配和重签名
        bundle_id: 可选，指定微信 Bundle ID
        container_id: 可选，指定微信容器 ID（目前用于日志）

    Returns:
        dict: salt_hex -> enc_key_hex 映射
    """
    import json

    binary = _find_binary()
    explicit_app_target = bool(app_path or bundle_id)
    resolved_app_path = _resolve_app_path(app_path, bundle_id)
    if explicit_app_target and not resolved_app_path and not pid:
        raise RuntimeError(
            f"未找到 Bundle ID {bundle_id} 对应的微信应用，请通过 --app-path 或 --pid 指定。"
        )
    selected_pid = _select_pid(pid, resolved_app_path, strict=explicit_app_target)

    # C 二进制的工作目录需要是 db_storage 的父目录
    work_dir = os.path.dirname(db_dir)
    if not os.path.isdir(work_dir):
        raise RuntimeError(f"微信数据目录不存在: {work_dir}")

    print(f"[+] 使用 C 二进制提取密钥: {binary}")
    print(f"[+] 工作目录: {work_dir}")
    print(f"[+] 数据目录: {db_dir}")
    if container_id:
        print(f"[+] 容器 ID: {container_id}")
    if resolved_app_path:
        print(f"[+] 微信应用: {resolved_app_path}")
    if selected_pid:
        print(f"[+] 微信进程 PID: {selected_pid}")

    args = [binary, "--db-dir", db_dir]
    if selected_pid:
        args.extend(["--pid", str(selected_pid)])

    try:
        result = subprocess.run(
            args,
            cwd=work_dir,
            capture_output=True,
            text=True,
            timeout=120,
        )
    except subprocess.TimeoutExpired:
        raise RuntimeError("密钥提取超时（120s）")
    except PermissionError:
        raise RuntimeError(
            f"无法执行 {binary}\n"
            "请确保文件有执行权限: chmod +x " + binary
        )

    # 打印 C 二进制的输出
    if result.stdout:
        print(result.stdout)
    if result.stderr:
        print(result.stderr, file=sys.stderr)

    # 检测 task_for_pid 失败 → 尝试 re-sign
    combined_output = (result.stdout or "") + (result.stderr or "")
    if "task_for_pid" in combined_output:
        print("\n[!] task_for_pid 失败：macOS 安全策略阻止了进程内存访问。")
        print("[!] 需要对微信重新签名以允许调试访问（不影响微信正常功能）。")

        ok, err = _resign_wechat(resolved_app_path, bundle_id)
        if ok:
            raise RuntimeError(
                "已对微信重新签名（保留原有权限）。请执行以下步骤后重试：\n"
                "  1. 退出微信（完全退出，不是最小化）\n"
                "  2. 重新打开微信并登录\n"
                "  3. 再次执行刚才的 sudo wechat-cli init 命令"
            )
        else:
            # 手动命令也需要保留原有权限
            manual_app = resolved_app_path or "/Applications/WeChat.app"
            quoted_app = shlex.quote(manual_app)
            raise RuntimeError(
                f"自动签名失败: {err}\n"
                "请手动执行以下命令后重试：\n"
                "  # 1. 提取微信原有权限\n"
                f"  codesign -d --entitlements wechat_ent.plist {quoted_app}\n"
                "  # 2. 用 PlistBuddy 添加 get-task-allow\n"
                '  /usr/libexec/PlistBuddy -c "Add :com.apple.security.get-task-allow bool true" wechat_ent.plist\n'
                "  # 3. 重新签名\n"
                f"  codesign --force --sign - --entitlements wechat_ent.plist {quoted_app}\n"
                "  # 4. 清理\n"
                "  rm wechat_ent.plist\n"
                "然后重启微信，再执行: sudo wechat-cli init"
            )

    # C 二进制输出 all_keys.json 到 work_dir
    c_output = os.path.join(work_dir, "all_keys.json")
    if not os.path.exists(c_output):
        raise RuntimeError(
            "C 二进制未能生成密钥文件。\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}"
        )

    # 读取并转存到 output_path
    with open(c_output, encoding="utf-8") as f:
        keys_data = json.load(f)

    raw_hex_keys = keys_data.pop("_raw_hex_keys", [])
    if raw_hex_keys:
        db_files, _ = collect_db_files(db_dir)
        existing_rels = {
            rel for rel, info in keys_data.items()
            if isinstance(info, dict) and "enc_key" in info
        }
        added = 0
        print(f"[+] 校验 {len(raw_hex_keys)} 个原始候选密钥...")
        for rel, path, sz, salt_hex, page1 in db_files:
            if rel in existing_rels:
                continue
            for enc_key_hex in raw_hex_keys:
                if not isinstance(enc_key_hex, str) or len(enc_key_hex) != 64:
                    continue
                try:
                    enc_key = bytes.fromhex(enc_key_hex)
                except ValueError:
                    continue
                if verify_enc_key(enc_key, page1):
                    keys_data[rel] = {
                        "enc_key": enc_key_hex,
                        "salt": salt_hex,
                        "size_mb": round(sz / 1024 / 1024, 1),
                    }
                    existing_rels.add(rel)
                    added += 1
                    print(f"  [FOUND] {rel} (raw key verified)")
                    break
        if added:
            print(f"[+] 通过原始候选密钥补充 {added} 个数据库密钥")

    with open(output_path, 'w', encoding='utf-8') as f:
        json.dump(keys_data, f, indent=2, ensure_ascii=False)

    # 清理 C 二进制的临时输出
    if os.path.abspath(c_output) != os.path.abspath(output_path):
        os.remove(c_output)

    # 构建 salt -> key 映射
    key_map = {}
    for rel, info in keys_data.items():
        if isinstance(info, dict) and "enc_key" in info and "salt" in info:
            key_map[info["salt"]] = info["enc_key"]

    print(f"\n[+] 提取到 {len(key_map)} 个密钥，保存到: {output_path}")
    return key_map
