"""
Rscholar API 客户端示例脚本

用法:
  1. 直接运行: python api_request.py
  2. 通过环境变量指定服务器: RSCHOLAR_URL=http://host:port python api_request.py

API 文档参考: POST /tasks -> GET /tasks/{id} -> GET /tasks/{id}/download
"""

import os
import sys
import time
import json
import requests

# ============================================================
# 可配置项
# ============================================================

# 服务器地址（优先读取环境变量 RSCHOLAR_URL）
BASE_URL = os.environ.get("RSCHOLAR_URL", "http://127.0.0.1:3000")
TASKS_URL = f"{BASE_URL}/tasks"

# 轮询间隔（秒）
POLL_INTERVAL = 3

# ============================================================
# 搜索任务定义
# 每个元组: (keyword, content_help, 额外参数 dict)
# ============================================================
SEARCH_TASKS: list[tuple[str, str, dict]] = [
    (
        "measure while drilling data",
        "我需要关于机器学习预测岩石强度的论文，"
        "是使用钻机钻进时测量的数据进行预测的，"
        "可以是震动、钻速、扭矩等数据来预测岩石的强度、内摩擦角、可钻性等信息",
        {
            "ylo": 2021,
            "sciif": 5.0,       # Impact Factor >= 5.0
            # "jci": None,      # JCI 过滤（可选）
            # "sci": "Q1",      # SCI 分区过滤（可选）
            # "source_include": ["openalex", "pubmed"],  # 来源白名单（可选）
            # "source_exclude": [],                      # 来源黑名单（可选）
        },
    ),
    # 在此添加更多搜索任务...
    # ("another keyword", "描述...", {"ylo": 2022}),
]


def create_task(keyword: str, content_help: str, extra: dict) -> str | None:
    """提交搜索任务，返回 task_id"""
    payload = {
        "keyword": keyword,
        "content_help": content_help,
        **extra,
    }
    try:
        resp = requests.post(TASKS_URL, json=payload, timeout=30)
        resp.raise_for_status()
        data = resp.json()
        task_id = data["task_id"]
        eta = data.get("eta_seconds", "?")
        print(f"  ✓ Task created: {task_id}  (ETA ~{eta}s)")
        return task_id
    except requests.RequestException as e:
        print(f"  ✗ Failed to create task: {e}", file=sys.stderr)
        return None


def poll_tasks(tracker: dict[str, dict]) -> None:
    """轮询所有未完成任务直到全部结束"""
    while True:
        pending = {tid: info for tid, info in tracker.items()
                   if info["status"] not in ("completed", "failed")}
        if not pending:
            break

        print(f"\n[{time.strftime('%H:%M:%S')}] Polling {len(pending)} pending task(s)...")

        for tid, info in pending.items():
            try:
                resp = requests.get(f"{TASKS_URL}/{tid}", timeout=30)
                resp.raise_for_status()
                data = resp.json()
            except requests.RequestException as e:
                print(f"  {tid} | Connection Error: {e}")
                continue

            status = data.get("status", "unknown")
            progress = data.get("progress", {})
            info["status"] = status

            step = progress.get("step", "")
            percent = progress.get("percent", 0)
            print(f"  {tid} | {info['keyword']}: {status} ({step} {percent}%)")

            if status == "completed":
                result = data.get("result", {})
                info["total_papers"]    = result.get("total_papers", 0)
                info["filtered_papers"] = result.get("filtered_papers", 0)
                info["csv_path"]        = result.get("csv_path")
                info["source_counts"]   = result.get("source_counts", {})
                info["source_errors"]   = result.get("source_errors", {})
                info["papers"]          = result.get("data", [])

                print(f"    ✓ Found {info['total_papers']} papers "
                      f"(filtered: {info['filtered_papers']})")
                if info["source_counts"]:
                    print(f"    Sources: {json.dumps(info['source_counts'], ensure_ascii=False)}")
                if info["source_errors"]:
                    print(f"    Source Errors: {json.dumps(info['source_errors'], ensure_ascii=False)}")

                # 预览前 2 条
                for i, paper in enumerate(info["papers"][:2]):
                    title = paper.get("title", "N/A")
                    doi   = paper.get("doi", "N/A")
                    print(f"    [{i+1}] {title}  (DOI: {doi})")

            elif status == "failed":
                info["error"] = data.get("error", "Unknown error")
                print(f"    ✗ Error: {info['error']}")

        time.sleep(POLL_INTERVAL)


def print_summary(tracker: dict[str, dict]) -> None:
    """输出最终汇总"""
    print("\n" + "=" * 60)
    print("RESULTS SUMMARY")
    print("=" * 60)

    for tid, info in tracker.items():
        status_icon = "✓" if info["status"] == "completed" else "✗"
        print(f"\n{status_icon} [{info['keyword']}]")
        print(f"  Status:          {info['status']}")

        if info["status"] == "completed":
            print(f"  Total Papers:    {info['total_papers']}")
            print(f"  Filtered Papers: {info['filtered_papers']}")
            print(f"  CSV Download:    {TASKS_URL}/{tid}/download")
            print(f"  BibTeX Download: {TASKS_URL}/{tid}/bibtex")
        elif info["status"] == "failed":
            print(f"  Error: {info.get('error', 'N/A')}")


def main() -> None:
    print(f"Rscholar API Client")
    print(f"Server: {BASE_URL}\n")

    # 1. 提交所有任务
    tracker: dict[str, dict] = {}
    for keyword, content_help, extra in SEARCH_TASKS:
        print(f"Creating task: [{keyword}]")
        task_id = create_task(keyword, content_help, extra)
        if task_id:
            tracker[task_id] = {
                "keyword":         keyword,
                "status":          "pending",
                "total_papers":    0,
                "filtered_papers": 0,
                "papers":          [],
                "csv_path":        None,
                "source_counts":   {},
                "source_errors":   {},
            }

    if not tracker:
        print("No tasks created. Exiting.", file=sys.stderr)
        sys.exit(1)

    # 2. 轮询等待结果
    poll_tasks(tracker)

    # 3. 汇总输出
    print_summary(tracker)


if __name__ == "__main__":
    main()
