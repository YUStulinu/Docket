"""Evaluation of Docket on the synthetic documents, through the real programs: docket-ocr (C++) for
the scans and the docket CLI (Rust) for extraction and questions, with the real local model.

    python -m docket_tools.evaluate ocr        page detection and OCR quality, by clean-up mode
    python -m docket_tools.evaluate extract    kinds, deadlines and fields: rules alone, and with the model
    python -m docket_tools.evaluate ask        retrieval and answers to the 48 questions, with their citations
    python -m docket_tools.evaluate all

Results go to tools/eval/*.json. The expected answers come from truth.json, never from the
programs under test.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import time
import unicodedata
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "testdata" / "synthetic"
RESULTS = ROOT / "tools" / "eval"
WORK = ROOT / "build" / "eval"
EXE = ".exe" if os.name == "nt" else ""
MODEL = ROOT / "models" / "Qwen2.5-3B-Instruct-Q4_K_M.gguf"


def find(*candidates: Path) -> str:
    for c in candidates:
        if c.exists():
            return str(c)
    sys.exit(f"missing: {candidates[0]} (build it first, see the README)")


def ocr_binary() -> str:
    b = ROOT / "native" / "build"
    return find(b / "Release" / f"docket-ocr{EXE}", b / f"docket-ocr{EXE}")


def docket_binary() -> str:
    return find(ROOT / "core" / "target" / "release" / f"docket{EXE}")


def truth() -> dict:
    return json.loads((DATA / "truth.json").read_text(encoding="utf-8"))


# --- text comparison ---------------------------------------------------------------------------


def norm(s: str) -> str:
    """For CER: one space between words, and the cedilla letters (ş ţ) that Tesseract's Romanian
    model writes taken as the comma letters (ș ț) the documents use."""
    s = s.replace("ş", "ș").replace("ţ", "ț").replace("Ş", "Ș").replace("Ţ", "Ț")
    return " ".join(s.split())


def fold(s: str) -> str:
    s = unicodedata.normalize("NFD", s.lower())
    return "".join(c for c in s if not unicodedata.combining(c))


def edits(a: str, b: str) -> int:
    if len(a) < len(b):
        a, b = b, a
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i] + [0] * len(b)
        for j, cb in enumerate(b, 1):
            cur[j] = min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb))
        prev = cur
    return prev[-1]


def cer(ref: str, hyp: str) -> float:
    r, h = norm(ref), norm(hyp)
    return edits(r, h) / max(1, len(r))


def wer(ref: str, hyp: str) -> float:
    r, h = norm(ref).split(), norm(hyp).split()
    # Word-level edit distance (same DP over lists).
    prev = list(range(len(h) + 1))
    for i, wr in enumerate(r, 1):
        cur = [i] + [0] * len(h)
        for j, wh in enumerate(h, 1):
            cur[j] = min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (wr != wh))
        prev = cur
    return prev[-1] / max(1, len(r))


# --- OCR ---------------------------------------------------------------------------------------

MODES = {
    "raw": ["--no-detect", "--cleanup", "gray"],  # the photo as it is
    "warp": ["--cleanup", "gray"],  # page found and straightened
    "warp+flat": ["--cleanup", "flat"],  # ... and the lighting flattened (Docket's default)
    "warp+binary": ["--cleanup", "binary"],  # ... and thresholded
    "warp+flat/psm4": ["--cleanup", "flat", "--psm", "4"],  # one column of text instead of automatic layout
}


def run_ocr(doc: dict, mode: str, out: Path) -> dict:
    photos = [str(DATA / p["photo"]) for p in doc["pages"]]
    tessdata = ROOT / "native" / "third_party" / "tessdata"
    t = time.time()
    subprocess.run([ocr_binary(), "--tessdata", str(tessdata), "--out", str(out), "--name", doc["id"], *MODES[mode], *photos],
                   check=True, capture_output=True)
    res = json.loads((out / f"{doc['id']}.json").read_text(encoding="utf-8"))
    res["seconds"] = time.time() - t
    return res


def corner_error(page_truth: dict, page: dict) -> float:
    """Mean distance of the four corners, as a share of the page's diagonal in the photo."""
    tc, pc = page_truth["corners"], page["corners"]
    diag = ((tc[0][0] - tc[2][0]) ** 2 + (tc[0][1] - tc[2][1]) ** 2) ** 0.5
    return statistics.mean(((a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2) ** 0.5 for a, b in zip(tc, pc)) / diag


def eval_ocr() -> dict:
    t = truth()
    results = {}
    for mode in MODES:
        out = WORK / "ocr" / mode
        out.mkdir(parents=True, exist_ok=True)
        cers, wers, corners, found, secs, rows = [], [], [], [], [], []
        for doc in t["documents"]:
            res = run_ocr(doc, mode, out)
            for pt, p in zip(doc["pages"], res["pages"]):
                c, w = cer(pt["text"], p["text"]), wer(pt["text"], p["text"])
                cers.append(c)
                wers.append(w)
                secs.append((p["ms_scan"] + p["ms_ocr"]) / 1000)
                if mode != "raw" and not doc["flat"]:
                    e = corner_error(pt, p)
                    corners.append(e)
                    found.append(e < 0.02)
                rows.append({"page": pt["photo"], "flat": doc["flat"], "cer": round(c, 4), "wer": round(w, 4)})
        results[mode] = {
            "pages": len(cers),
            "cer_mean": statistics.mean(cers),
            "cer_median": statistics.median(cers),
            "wer_mean": statistics.mean(wers),
            "pages_cer_under_5pct": sum(c < 0.05 for c in cers) / len(cers),
            "seconds_per_page": statistics.mean(secs),
            "page_found": (sum(found) / len(found)) if found else None,
            "corner_error_mean": statistics.mean(corners) if corners else None,
            "by_page": rows,
        }
        r = results[mode]
        print(f"{mode:12} CER {r['cer_mean']:.2%} (median {r['cer_median']:.2%})  WER {r['wer_mean']:.2%}  "
              f"pages under 5% CER {r['pages_cer_under_5pct']:.0%}  {r['seconds_per_page']:.2f} s/page"
              + (f"  page found {r['page_found']:.0%}, corner error {r['corner_error_mean']:.2%} of the diagonal" if found else ""))
    return results


# --- the model ---------------------------------------------------------------------------------


class LlamaServer:
    """A llama-server for the evaluation, on a free port, stopped at the end."""

    def __init__(self, ctx: int = 8192):
        self.ctx = ctx
        self.proc = None

    def __enter__(self) -> str:
        binary = os.environ.get("DOCKET_LLAMA_SERVER") or find(ROOT / "tools" / "llama.cpp" / f"llama-server{EXE}")
        if not MODEL.exists():
            sys.exit("the model is missing: run scripts/setup-model.sh")
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        log = open(WORK / "llama-server.log", "w")
        self.proc = subprocess.Popen([binary, "-m", str(MODEL), "--host", "127.0.0.1", "--port", str(port), "-c", str(self.ctx),
                                      "--parallel", "1", "--no-webui"], stdout=log, stderr=log)
        url = f"http://127.0.0.1:{port}"
        for _ in range(600):
            try:
                with urllib.request.urlopen(url + "/health", timeout=2) as r:
                    if r.status == 200:
                        return url
            except Exception:
                time.sleep(0.5)
        raise RuntimeError("llama-server did not start (see build/eval/llama-server.log)")

    def __exit__(self, *exc):
        if self.proc:
            self.proc.terminate()
            self.proc.wait(10)


def docket(args: list[str], llm: str | None, env: dict | None = None) -> str:
    cmd = [docket_binary(), *args, "--today", truth()["today"]]
    if llm:
        cmd += ["--llm", llm]
    out = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", env={**os.environ, **(env or {})})
    if out.returncode != 0:
        raise RuntimeError(f"docket {' '.join(args)}: {out.stderr.strip()}")
    return out.stdout


# --- extraction --------------------------------------------------------------------------------


def field_found(expected: str, fields: list[dict]) -> bool:
    e = re.sub(r"[^0-9a-z]", "", fold(expected))
    for f in fields:
        v = re.sub(r"[^0-9a-z]", "", fold(f["value"]))
        if v and (e in v or v in e) and min(len(v), len(e)) >= 0.6 * max(len(v), len(e)):
            return True
    return False


def field_list(got: dict) -> list[dict]:
    """Fields as a list, whether checked (a list) or the model's raw answer (an object by key)."""
    f = got.get("fields", [])
    return [{"key": k, **v} for k, v in f.items()] if isinstance(f, dict) else f


def invented(got: dict, text: str) -> int:
    """Field values that are nowhere in the document's text (made up)."""
    t = re.sub(r"[^0-9a-z]", "", fold(text))
    n = 0
    for f in field_list(got):
        v = re.sub(r"[^0-9a-z]", "", fold(f.get("value", "")))
        if v and v not in t:
            n += 1
    return n


def score_extraction(doc: dict, got: dict) -> dict:
    want = {(d["kind"], d["date"]) for d in doc["deadlines"]}
    have = {(d["kind"], d["date"]) for d in got.get("deadlines", [])}
    have_dates = {d["date"] for d in got.get("deadlines", [])}
    fields = {k: field_found(v, field_list(got)) for k, v in doc["fields"].items()}
    notice_ok = all(
        any(g["kind"] == "contract_end" and g.get("notice_days") == d["notice_days"] for g in got["deadlines"])
        for d in doc["deadlines"] if d.get("notice_days")
    )
    return {
        "kind": got["kind"] == doc["kind"],
        "deadlines": len(want & have) / len(want),
        "deadline_dates": sum(d in have_dates for _, d in want) / len(want),
        "extra_deadlines": len(have - want),
        "fields": sum(fields.values()) / max(1, len(fields)),
        "missed_fields": [k for k, ok in fields.items() if not ok],
        "notice": notice_ok,
        "rejected": len(got["extraction"]["rejected"]),
        "seconds": got["extraction"]["seconds"],
    }


def eval_extract(llm: str | None) -> dict:
    t = truth()
    src = WORK / "ocr" / "warp+flat"
    if not (src / f"{t['documents'][0]['id']}.json").exists():
        sys.exit("run the OCR evaluation first (python -m docket_tools.evaluate ocr)")
    perfect = WORK / "perfect"
    perfect.mkdir(parents=True, exist_ok=True)
    configs = [("rules", None)] + ([("model-unchecked", llm), ("model", llm)] if llm else [])
    results = {}
    for name, url in configs:
        for text in ["ocr", "perfect"]:
            rows = []
            for doc in t["documents"]:
                path = src / f"{doc['id']}.json"
                if text == "perfect":
                    path = perfect / f"{doc['id']}.json"
                    path.write_text(json.dumps({"pages": [{"text": p["text"]} for p in doc["pages"]]}, ensure_ascii=False), encoding="utf-8")
                extra = ["--unchecked"] if name == "model-unchecked" else []
                got = json.loads(docket(["extract", str(path), *extra], url))
                pages_text = "\n".join(p["text"] for p in json.loads(path.read_text(encoding="utf-8"))["pages"])
                rows.append({"document": doc["id"], **score_extraction(doc, got), "invented": invented(got, pages_text), "got": got})
            key = f"{name}/{text}"
            r = results[key] = {
                "documents": len(rows),
                "kind": statistics.mean(r["kind"] for r in rows),
                "deadlines": statistics.mean(r["deadlines"] for r in rows),
                "deadline_dates": statistics.mean(r["deadline_dates"] for r in rows),
                "extra_deadlines": sum(r["extra_deadlines"] for r in rows),
                "fields": statistics.mean(r["fields"] for r in rows),
                "notice": statistics.mean(r["notice"] for r in rows if r["document"].startswith("rent")),
                "rejected": sum(r["rejected"] for r in rows),
                "invented_values": sum(r["invented"] for r in rows),
                "seconds_mean": statistics.mean(r["seconds"] for r in rows),
                "by_document": rows,
            }
            print(f"{key:23} kind {r['kind']:.0%}  deadlines {r['deadlines']:.0%} (dates {r['deadline_dates']:.0%}, {r['extra_deadlines']} extra)  "
                  f"fields {r['fields']:.0%}  invented {r['invented_values']}  rejected {r['rejected']}  {r['seconds_mean']:.1f} s/doc")
            for row in rows:
                if row["deadlines"] < 1 or not row["kind"]:
                    print(f"    {row['document']}: kind {row['got']['kind']}, deadlines {[(d['kind'], d['date']) for d in row['got']['deadlines']]}")
    return results


# --- questions ---------------------------------------------------------------------------------


def answer_ok(q: dict, text: str) -> bool:
    if "date" in q:
        d = dt.date.fromisoformat(q["date"])
        forms = {d.isoformat(), d.strftime("%d.%m.%Y"), f"{d.day}.{d.month:02d}.{d.year}", d.strftime("%d/%m/%Y")}
        months = ["ianuarie", "februarie", "martie", "aprilie", "mai", "iunie", "iulie", "august", "septembrie", "octombrie", "noiembrie", "decembrie"]
        forms |= {f"{d.day} {months[d.month - 1]} {d.year}", f"{d.day:02d} {months[d.month - 1]} {d.year}"}
        t = fold(text)
        return any(fold(f) in t for f in forms)
    return fold(q["text"]) in fold(text)


def eval_ask(llm: str | None) -> dict:
    t = truth()
    src = WORK / "ocr" / "warp+flat"
    vault = Path(tempfile.mkdtemp(prefix="docket-eval-"))
    shutil.rmtree(vault)
    env = {"DOCKET_PASSPHRASE": "evaluation"}
    docket(["init", str(vault)], None, env)
    # Ids are random: map them back to the truth's documents by order of adding.
    ids = {}
    for doc in t["documents"]:
        line = docket(["add", str(vault), str(src / f"{doc['id']}.json")], llm, env)
        ids[line.split()[0]] = doc["id"]
    results = {}
    configs = [("search", None)] + ([("model", llm)] if llm else [])
    for name, url in configs:
        rows = []
        for q in t["questions"]:
            row = {"question": q["question"], "document": q["document"], "page": q["page"]}
            if name == "search":
                hits = [ids.get(h["document_id"]) for h in json.loads(docket(["search", str(vault), q["question"], "--json"], None, env))]
                accepted = [q["document"], *q.get("also", [])]
                row["found_first"] = bool(hits) and hits[0] in accepted
                row["found_top3"] = any(h in accepted for h in hits[:3])
                ans = json.loads(docket(["ask", str(vault), q["question"], "--locale", "ro"], None, env))
            else:
                ans = json.loads(docket(["ask", str(vault), q["question"], "--locale", "ro"], url, env))
            cites = [(ids.get(c["document_id"]), c["page"]) for c in ans["citations"]]
            row.update({
                "answer": ans["text"],
                "verified": ans["verified"],
                "correct": answer_ok(q, ans["text"]),
                "cites_document": any(d in [q["document"], *q.get("also", [])] for d, _ in cites),
                "cites_page": (q["document"], q["page"]) in cites or any(d in q.get("also", []) for d, _ in cites),
                "citations": cites,
            })
            if name == "search":
                row["hits"] = hits
            rows.append(row)
        r = results[name] = {
            "questions": len(rows),
            "correct": statistics.mean(r["correct"] for r in rows),
            "cites_document": statistics.mean(r["cites_document"] for r in rows),
            "cites_page": statistics.mean(r["cites_page"] for r in rows),
            "correct_and_cited": statistics.mean(r["correct"] and r["cites_page"] for r in rows),
            "wrong_but_verified": sum((not r["correct"]) and r["verified"] for r in rows),
            "by_question": rows,
        }
        if name == "search":
            r["retrieval_first"] = statistics.mean(x["found_first"] for x in rows)
            r["retrieval_top3"] = statistics.mean(x["found_top3"] for x in rows)
            print(f"retrieval: the right document first {r['retrieval_first']:.0%}, in the top 3 {r['retrieval_top3']:.0%}")
        print(f"{name:7} answer right {r['correct']:.0%}  cites the document {r['cites_document']:.0%}  the page {r['cites_page']:.0%}  "
              f"right and cited {r['correct_and_cited']:.0%}  wrong but marked verified {r['wrong_but_verified']}")
        for row in rows:
            if not (row["correct"] and row["cites_page"]):
                print(f"    {row['question']} -> {row['answer'][:110]!r} {row['citations']}")
    shutil.rmtree(vault, ignore_errors=True)
    return results


def save(name: str, data: dict) -> None:
    RESULTS.mkdir(parents=True, exist_ok=True)
    data = {"date": dt.date.today().isoformat(), **data}
    (RESULTS / f"{name}.json").write_text(json.dumps(data, indent=1, ensure_ascii=False), encoding="utf-8", newline="\n")


def main() -> None:
    sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("what", choices=["ocr", "extract", "ask", "all"])
    ap.add_argument("--no-llm", action="store_true", help="rules and search only")
    a = ap.parse_args()
    WORK.mkdir(parents=True, exist_ok=True)
    if a.what in ("ocr", "all"):
        save("ocr", eval_ocr())
    if a.what in ("extract", "ask", "all"):
        if a.no_llm:
            if a.what in ("extract", "all"):
                save("extract", eval_extract(None))
            if a.what in ("ask", "all"):
                save("ask", eval_ask(None))
            return
        with LlamaServer() as url:
            if a.what in ("extract", "all"):
                save("extract", {"model": MODEL.name, **eval_extract(url)})
            if a.what in ("ask", "all"):
                save("ask", {"model": MODEL.name, **eval_ask(url)})


if __name__ == "__main__":
    main()
