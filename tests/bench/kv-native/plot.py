#!/usr/bin/env python3
# Copyright 2018-2026 the Deno authors. MIT license.
"""Create Native-only exportable figures (requires matplotlib)."""
import json
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

root = Path(__file__).resolve().parent
summary = json.loads((root / "summary.json").read_text())
growth = sorted(summary["growth"], key=lambda r: r["updates"])
fig, axes = plt.subplots(1, 3, figsize=(16, 4.5), constrained_layout=True)
for mode, shift, color in [("memory", -.18, "#37a083"), ("disk", .18, "#2364aa")]:
    rows = [r for r in summary["workloads"] if r["entries"] == 1000 and r["mode"] == mode]
    by_operation = {r["operation"]: r for r in rows}
    values = [by_operation[name]["p50_ms"] for name in ["get", "overwrite", "cas", "list_100"]]
    bars = axes[0].bar([i + shift for i in range(4)], values, width=.34, color=color, label=mode.capitalize())
    axes[0].bar_label(bars, labels=[f"{v:.3g}" for v in values], padding=3, fontsize=9)
axes[0].set_xticks(range(4), ["Get", "Overwrite", "CAS", "List 100"])
axes[0].set_yscale("log")
all_values = [r["p50_ms"] for r in summary["workloads"] if r["entries"] == 1000]
axes[0].set_ylim(min(all_values) / 2, max(all_values) * 3)
axes[0].set_ylabel("p50 latency (ms, log scale)")
axes[0].set_title("Native operations: 1,000 entries")
axes[0].legend(frameon=False)
updates = [r["updates"] for r in growth]
axes[1].plot(updates, [r["read_p50_ms"] for r in growth], "o-", color="#2364aa")
axes[1].set_ylabel("Get p50 (ms)")
axes[1].set_ylim(0, max(r["read_p50_ms"] for r in growth) * 1.3)
axes[1].set_title("Read latency as history grows")
axes[2].plot(updates, [r["file_bytes"] / 1024**2 for r in growth], "o-", color="#2364aa")
axes[2].set_ylabel("Journal size (MiB)")
axes[2].set_ylim(0, max(r["file_bytes"] for r in growth) / 1024**2 * 1.3)
axes[2].set_title("File growth at constant entry count")
for ax in axes:
    ax.grid(axis="y", alpha=.2)
    ax.set_axisbelow(True)
    ax.spines[["top", "right"]].set_visible(False)
for ax in axes[1:]:
    ax.set_xlabel("Overwrites since seed")
profile = summary["metadata"].get("cargo_profile", summary["metadata"]["profile"])
fig.suptitle(f"Oden Native KV backend — {profile} build, 256-byte values, warm cache", fontsize=13)
fig.savefig(root / "comparison.png", dpi=180)
fig.savefig(root / "comparison.svg")
svg = root / "comparison.svg"
svg.write_text("\n".join(line.rstrip() for line in svg.read_text().splitlines()) + "\n")
print("Native-only PNG and SVG saved.")
