#!/usr/bin/env python3
# Copyright 2018-2026 the Deno authors. MIT license.
"""Publish Native samples and SQLite-relative ratios, without SQLite raw timings."""
import argparse
import collections
import json
import math
from pathlib import Path
import statistics

ROOT = Path(__file__).resolve().parent
parser = argparse.ArgumentParser()
parser.add_argument('--input', type=Path, required=True)
parser.add_argument('--profile', choices=['debug', 'release-lite'], required=True)
parser.add_argument('--api-input', type=Path)
args = parser.parse_args()


def percentile(samples, fraction):
    ordered = sorted(samples)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def summarize(path, expected_groups):
    rows = [json.loads(line) for line in path.read_text().splitlines()]
    meta = rows[0]
    assert meta['kind'] == 'metadata'
    groups = collections.defaultdict(dict)
    for row in rows[1:]:
        if row['kind'] == 'workload':
            key = (row['entries'], row['mode'], row['operation'])
            groups[key].setdefault(row['backend'], []).append(row)
    assert len(groups) == expected_groups
    summaries = []
    for (count, mode, operation), backends in sorted(groups.items()):
        runs = backends['native']
        assert len(runs) == meta['repeats']
        assert {r['repeat'] for r in runs} == set(range(meta['repeats']))
        values = [v for r in runs for v in r['latency_ms']]
        assert len(values) == meta['iterations'] * meta['repeats']
        row = dict(entries=count, mode=mode, operation=operation,
                   p50_ms=percentile(values, .5), p95_ms=percentile(values, .95),
                   ops_per_s=statistics.median(r['ops_per_s'] for r in runs))
        if 'initial_file_bytes' in runs[0]:
            row['initial_bytes'] = statistics.median(r['initial_file_bytes'] for r in runs)
            row['final_bytes'] = statistics.median(r['final_file_bytes'] for r in runs)
        if 'sqlite' in backends:
            baseline = backends['sqlite']
            assert len(baseline) == meta['repeats']
            assert {r['repeat'] for r in baseline} == set(range(meta['repeats']))
            samples = [v for r in baseline for v in r['latency_ms']]
            assert len(samples) == len(values)
            row['sqlite_relative_p50_speedup'] = percentile(samples, .5) / row['p50_ms']
            row['sqlite_relative_p95_speedup'] = percentile(samples, .95) / row['p95_ms']
            row['paired_repeat_p50_speedups'] = [
                percentile(next(r for r in baseline if r['repeat'] == i)['latency_ms'], .5) /
                percentile(next(r for r in runs if r['repeat'] == i)['latency_ms'], .5)
                for i in range(meta['repeats'])]
        summaries.append(row)
    growth = [r for r in rows if r['kind'] == 'growth' and r['backend'] == 'native']
    for row in growth:
        row['read_p50_ms'] = percentile(row['read_latency_ms'], .5)
        row['read_p95_ms'] = percentile(row['read_latency_ms'], .95)
    native = [r for r in rows if r['kind'] == 'metadata' or r.get('backend') == 'native']
    return dict(metadata=meta, workloads=summaries, growth=growth), native


summary, native = summarize(args.input, 24)
assert len(summary['growth']) == 4
(ROOT / f'results-{args.profile}.jsonl').write_text(
    ''.join(json.dumps(row) + '\n' for row in native))
summary['metadata']['cargo_profile'] = args.profile
if args.api_input:
    api, api_rows = summarize(args.api_input, 8)
    summary['api'] = api
    (ROOT / 'results-api-debug.jsonl').write_text(
        ''.join(json.dumps(row) + '\n' for row in api_rows))
(ROOT / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')

report = [
    '# Oden Native KV ベンチマーク', '',
    '変更キーだけを保存する差分ログ、状態キャッシュ、変更分のundo、TTL索引、メモリ操作のworker往復削減を適用した後の測定。',
    '表・グラフ・生データはNativeのみ。SQLiteとの同条件比較は速度比で示す（SQLite p50 ÷ Native p50、1より大きければNativeが速い）。', '',
    '![Nativeベンチマーク](comparison.png)', '', '## 条件', '',
    '- Linux x86_64、AMD EPYC 9V74、3 vCPU、共有クラウド環境。/tmpのoverlay、温めたOSキャッシュ。',
    f'- Rust Databaseトレイトを直接呼ぶ`{args.profile}`ビルド。JavaScript/V8とキーエンコードはこの測定に含まない。',
    '- `release-lite`はリポジトリ既定の最適化プロファイル（Native/SQLite Rustコードはopt-level=z、thin LTO）。',
    '- 値256バイト、キーは4バイトのbig-endian整数。100件・1,000件・10,000件、単一クライアントの逐次操作。',
    '- 各条件1,000操作×5回、各回新規DB。DB準備と5回の読み取りは除外。比較の実行順序は各回交互。',
    '- p50/p95は5,000サンプルのnearest-rank、ops/sは5回の中央値。',
    '- Nativeは毎コミットOSロック＋sync_all。SQLiteは既存のDeno互換実装、既定WAL設定。耐久性設定は変更していない。',
    '- CASは読み取り＋versionstampチェック付き更新、list_100は100件の値取得。',
    '- ビルド・テスト・他のベンチを同時実行していない。共有ホストの負荷は制御できず、全環境・全ワークロードでの優位を保証する測定ではない。', '',
    '## バックエンドのレイテンシー', '',
    '| 件数 | 保存 | 操作 | Native p50 (ms) | Native p95 (ms) | Native ops/s | SQLiteに対するp50速度比 |',
    '|---:|---|---|---:|---:|---:|---:|']
for row in summary['workloads']:
    ratio = row.get('sqlite_relative_p50_speedup')
    report.append(f'| {row["entries"]:,} | {row["mode"]} | {row["operation"]} | {row["p50_ms"]:.4f} | {row["p95_ms"]:.4f} | {row["ops_per_s"]:,.0f} | {ratio:.2f}× |' if ratio is not None else
                  f'| {row["entries"]:,} | {row["mode"]} | {row["operation"]} | {row["p50_ms"]:.4f} | {row["p95_ms"]:.4f} | {row["ops_per_s"]:,.0f} | — |')
ratios = [r['sqlite_relative_p50_speedup'] for r in summary['workloads'] if 'sqlite_relative_p50_speedup' in r]
if ratios:
    report += ['', f'中央値でNativeが速かった条件は{sum(r > 1 for r in ratios)}/{len(ratios)}。速度比は{min(ratios):.2f}〜{max(ratios):.2f}倍。p95と回ごとの速度比は集計JSONに収録。']
if 'api' in summary:
    report += ['', '## JavaScript APIも含む確認', '',
               '`Oden.openKv()`と`Deno.openKv()`を同じdebug CLIで呼ぶ別測定。キーは["bench", number]、値はUint8Array(256)、1,000件。',
               '各条件1,000操作×5回、100回の同じ操作でウォームアップ。型付き配列のV8シリアライズ、キーエンコード、JS Promise、一覧の非同期反復を含む。',
               'release-liteのRustトレイト測定と数値を直接比較できない。', '',
               '| 保存 | 操作 | Native p50 (ms) | Native p95 (ms) | SQLiteに対するp50速度比 |',
               '|---|---|---:|---:|---:|']
    for r in summary['api']['workloads']:
        ratio = r.get('sqlite_relative_p50_speedup')
        report.append(f'| {r["mode"]} | {r["operation"]} | {r["p50_ms"]:.4f} | {r["p95_ms"]:.4f} | {ratio:.2f}× |' if ratio is not None else
                      f'| {r["mode"]} | {r["operation"]} | {r["p50_ms"]:.4f} | {r["p95_ms"]:.4f} | — |')
report += ['', '## 更新履歴の影響', '',
           '同じ1,000件のDBに追加で100回上書き。各点のgetは10回、成長試験は1回。reopenは再オープン＋最初のget＋close。', '',
           '| 上書き回数 | ファイル (MiB) | get p50 (ms) | get p95 (ms) | reopen (ms) |',
           '|---:|---:|---:|---:|---:|']
for r in summary['growth']:
    report.append(f'| {r["updates"]} | {r["file_bytes"] / 1024**2:.3f} | {r["read_p50_ms"]:.4f} | {r["read_p95_ms"]:.4f} | {r["reopen_ms"]:.3f} |')
report += ['', '## 改善前と制約', '',
           '改善前のNative測定は[before/summary.json](before/summary.json)と[before/results-debug.jsonl](before/results-debug.jsonl)に保存。',
           '改善前はdebugビルド・30操作×3回なので、今回の最適化ビルドと直接の倍率比較はできない。',
           '改善前は全状態を各更新で保存し全履歴を毎操作で復元した。今回の通常読み取りは履歴を復元せず、更新は変更キーだけを追記する。',
           'ログ圧縮は未実装。起動時には全履歴を1回復元し、ログは更新量に応じて増える。生存データの索引はメモリに保持する。',
           '今回の比較は逐次CRUDが対象で、大きな値、多数の同時クライアント、長期履歴、キュー・watchの速度は未評価。', '',
           '## 再実行', '', '```sh',
           'cargo build -p deno_kv --profile release-lite --example kv_bench --features sqlite,deno_core/v8',
           'target/release-lite/examples/kv_bench --compare-sqlite > /tmp/kv-compare.jsonl',
           'target/debug/oden run --unstable-kv -A tests/bench/kv-native/api_bench.ts --compare-sqlite > /tmp/kv-api-compare.jsonl',
           'python tests/bench/kv-native/summarize.py --profile release-lite --input /tmp/kv-compare.jsonl --api-input /tmp/kv-api-compare.jsonl',
           'python tests/bench/kv-native/plot.py', '```', '',
           '比較用のSQLite生データは/tmpの一時ファイルにだけ出力。公開用の生データ・集計ではSQLiteの絶対値を除き、比較の速度比を残す。',
           '`kv_bench`・`api_bench.ts`は引数なしならNativeだけを実行する。グラフ生成にはmatplotlibが必要。', '',
           '- [Rust計測コード](../../../ext/kv/examples/kv_bench.rs)',
           '- [JS API計測コード](api_bench.ts)',
           f'- [Native生データ](results-{args.profile}.jsonl)',
           '- [JS API Native生データ](results-api-debug.jsonl)',
           '- [集計と速度比](summary.json)',
           '- [測定環境・ソース識別情報](environment.json)', '']
(ROOT / 'README.md').write_text('\n'.join(report))
print(f'Native samples published; {len(summary["workloads"])} backend groups summarized.')
