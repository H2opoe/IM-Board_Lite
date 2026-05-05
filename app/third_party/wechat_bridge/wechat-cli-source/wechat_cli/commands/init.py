"""init 命令 — 交互式初始化，提取密钥并生成配置"""

import json
import os
import sys

import click

from ..core.config import STATE_DIR, CONFIG_FILE, KEYS_FILE, auto_detect_db_dir


def _abs_path(path):
    return os.path.abspath(os.path.expanduser(path))


@click.command()
@click.option("--config", "config_path", default=None, help="配置文件路径")
@click.option("--keys-file", default=None, help="密钥文件路径")
@click.option("--db-dir", default=None, help="微信数据目录路径（默认自动检测）")
@click.option("--bundle-id", default=None, help="macOS 微信 Bundle ID，例如 com.tencent.xinWeChat2")
@click.option("--container-id", default=None, help="macOS 容器 ID（默认使用 --bundle-id）")
@click.option("--app-path", default=None, help="macOS 微信 .app 路径，用于多开进程匹配和重签名")
@click.option("--pid", type=int, default=None, help="指定微信进程 PID（多开时可用于精确选择）")
@click.option("--force", is_flag=True, help="强制重新提取密钥")
@click.pass_context
def init(ctx, config_path, keys_file, db_dir, bundle_id, container_id, app_path, pid, force):
    """初始化 wechat-cli：提取密钥并生成配置"""
    click.echo("WeChat CLI 初始化")
    click.echo("=" * 40)

    parent_config_path = None
    if ctx.parent is not None:
        parent_config_path = ctx.parent.params.get("config_path")
    config_path = _abs_path(config_path or parent_config_path or CONFIG_FILE)
    keys_file = _abs_path(keys_file or KEYS_FILE)

    # 1. 检查是否已初始化
    custom_instance = any([db_dir, bundle_id, container_id, app_path, pid])
    if os.path.exists(config_path) and os.path.exists(keys_file) and not force and not custom_instance:
        click.echo(f"已初始化（配置: {config_path}）")
        click.echo("使用 --force 重新提取密钥")
        return
    if os.path.exists(config_path) and os.path.exists(keys_file) and not force and custom_instance:
        click.echo("[*] 检测到指定了微信实例参数，将重新生成当前配置")

    # 2. 创建状态目录
    os.makedirs(STATE_DIR, exist_ok=True)
    os.makedirs(os.path.dirname(config_path), exist_ok=True)
    os.makedirs(os.path.dirname(keys_file), exist_ok=True)

    # 3. 确定 db_dir
    effective_container_id = container_id or bundle_id
    if db_dir is None:
        db_dir = auto_detect_db_dir(bundle_id=bundle_id, container_id=effective_container_id)
        if db_dir is None:
            click.echo("[!] 未能自动检测到微信数据目录", err=True)
            click.echo("请通过 --db-dir 参数指定，例如:", err=True)
            click.echo("  wechat-cli init --db-dir ~/path/to/db_storage", err=True)
            if effective_container_id:
                click.echo(f"  已尝试容器: ~/Library/Containers/{effective_container_id}", err=True)
            sys.exit(1)
        click.echo(f"[+] 检测到微信数据目录: {db_dir}")
    else:
        db_dir = _abs_path(db_dir)
        if not os.path.isdir(db_dir):
            click.echo(f"[!] 目录不存在: {db_dir}", err=True)
            sys.exit(1)
        click.echo(f"[+] 使用指定数据目录: {db_dir}")

    if app_path:
        app_path = _abs_path(app_path)
        if not os.path.isdir(app_path):
            click.echo(f"[!] 应用不存在: {app_path}", err=True)
            sys.exit(1)
        click.echo(f"[+] 使用指定微信应用: {app_path}")
    if bundle_id:
        click.echo(f"[+] 使用指定 Bundle ID: {bundle_id}")
    if effective_container_id:
        click.echo(f"[+] 使用指定容器 ID: {effective_container_id}")
    if pid:
        click.echo(f"[+] 使用指定进程 PID: {pid}")

    # 4. 提取密钥
    click.echo("\n开始提取密钥...")
    try:
        from ..keys import extract_keys
        key_map = extract_keys(
            db_dir,
            keys_file,
            pid=pid,
            app_path=app_path,
            bundle_id=bundle_id,
            container_id=effective_container_id,
        )
    except RuntimeError as e:
        click.echo(f"\n[!] 密钥提取失败: {e}", err=True)
        if "sudo" not in str(e).lower():
            click.echo("提示: macOS/Linux 可能需要 sudo 权限", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"\n[!] 密钥提取出错: {e}", err=True)
        sys.exit(1)

    # 5. 写入配置
    cfg = {
        "db_dir": db_dir,
        "keys_file": keys_file,
    }
    if bundle_id:
        cfg["wechat_bundle_id"] = bundle_id
    if effective_container_id:
        cfg["wechat_container_id"] = effective_container_id
    if app_path:
        cfg["wechat_app_path"] = app_path
    with open(config_path, "w", encoding="utf-8") as f:
        json.dump(cfg, f, indent=2, ensure_ascii=False)

    click.echo(f"\n[+] 初始化完成!")
    click.echo(f"    配置: {config_path}")
    click.echo(f"    密钥: {keys_file}")
    click.echo(f"    提取到 {len(key_map)} 个数据库密钥")
    click.echo("\n现在可以使用:")
    click.echo("  wechat-cli sessions")
    click.echo("  wechat-cli history \"联系人\"")
