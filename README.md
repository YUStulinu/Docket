# Docket

Your papers and their deadlines, on your phone and nowhere else. You photograph a document:

- the car's registration (talon) with its ITP stamp;
- an RCA or CASCO policy;
- a passport, an ID card or a driving licence;
- a warranty;
- a rent contract;
- a bill.

Docket straightens the photo and reads it. It finds the dates that matter and reminds you ahead of
them, at the lead time each kind of deadline needs:

- book the ITP two weeks before it expires;
- renew a passport long before a trip that needs six months of validity;
- give notice on a rent contract before the notice period starts.

You can also ask questions about your own documents, such as "când expiră garanția la mașina de
spălat?", and get an answer that cites the exact page.

Everything runs on the phone:

- **OCR:** OpenCV and Tesseract.
- **Language model:** Qwen2.5-3B-Instruct through llama.cpp.
- **Storage:** the documents, their text and their page images are encrypted on disk with a key that
  the Android Keystore protects.
- **Network:** the app has no internet permission at all.

| Part | Language |
| --- | --- |
| Android app: camera with a live page outline, gallery import, notifications, calendar export, Compose UI | Kotlin (Jetpack Compose) |
| Page detection, perspective and lighting correction, Tesseract, llama.cpp, the JNI bridge | C++ (OpenCV, Tesseract, llama.cpp) |
| The shared core: encrypted storage, extraction and its checks, deadlines and reminders, calendar, search and answers; bound to Kotlin through UniFFI | Rust |
| Synthetic documents and the evaluation harness | Python |

## Building it

You need:
- Rust;
- CMake and Ninja;
- the Android SDK with NDK 29.0.14206865, CMake 3.31.6 and platform 37;
- JDK 21;
- Python 3 with numpy and Pillow, for the tools.

For the desktop tools on Windows you also need Visual Studio 2022 and OpenCV 4.12; on Linux,
`libopencv-dev`.

```bash
bash scripts/fetch-deps.sh android          # pinned, checksummed: Leptonica, Tesseract, llama.cpp, OCR models, OpenCV for Android
bash scripts/build-deps.sh android arm64-v8a
bash scripts/build-deps.sh android x86_64   # for the emulator
rustup target add aarch64-linux-android x86_64-linux-android && cargo install cargo-ndk
cd android && ./gradlew assembleDebug       # also builds the Rust core (cargo-ndk) and its Kotlin bindings (UniFFI)
```

On Windows, keep the checkout in a short path (`C:\src\Docket`, not deep in a temporary folder):
MSVC's and CMake's test builds of the dependencies nest several folders deep and fail past the
260-character path limit.

`android/local.properties` points at the SDK (`sdk.dir=...`). The NDK's build tools dislike spaces in
paths: if your SDK is in such a folder (`C:\Android SDK`), point at it through its short name
(`C\:/ANDROI~1`).

**The model** (1.9 GB, not in the APK): run `bash scripts/setup-model.sh`. Then either:
- import the `.gguf` file from the phone's storage in Settings; or
- for development, copy it into the app's private storage:

```bash
adb push models/Qwen2.5-3B-Instruct-Q4_K_M.gguf /data/local/tmp/
adb shell run-as io.github.yustulinu.docket cp /data/local/tmp/Qwen2.5-3B-Instruct-Q4_K_M.gguf files/models/
```

Without the model, Docket still works: rules find the deadlines and search answers the questions.

**The desktop tools.** The same pipeline runs on the desktop, with the same native and Rust code:

- `docket-ocr` (C++) turns photos into OCR JSON;
- `docket` (Rust) is a CLI over the same core, with an encrypted vault protected by a passphrase;
- a local llama-server plays the model's part (`scripts/setup-llama.sh`).

```bash
bash scripts/fetch-deps.sh && bash scripts/build-deps.sh host
cmake -S native -B native/build -DCMAKE_PREFIX_PATH="$PWD/native/third_party/install/host;<OpenCV>" && cmake --build native/build --config Release
cd core && cargo build --release --features cli
native/build/Release/docket-ocr --tessdata native/third_party/tessdata --out scans --name rca photo.jpg
core/target/release/docket init my-vault && core/target/release/docket add my-vault scans/rca.json
core/target/release/docket reminders my-vault --locale ro
```

## How it works

### Scanning (C++)

`native/src/scan.cpp`:

1. **Finding the page.** The photo is shrunk to 800 px. Two kinds of candidate shapes are taken from
   it: the edges (Canny, with thresholds from the median brightness) and the bright regions (Otsu,
   because paper is lighter than the table). Each contour is reduced to its convex hull and then to
   four corners. The candidate kept is the one that is largest and closest to a rectangle; angles
   far from square disqualify a candidate. The phone runs the same detection on camera frames, a few
   times a second, to draw the live outline.
2. **Perspective.** The corners are mapped onto an upright rectangle of the page's own proportions,
   about 200 DPI on the long side. 1.5% of the margin is cut off, so that no sliver of the table
   reaches the OCR as "|".
3. **Light.** The page's background is estimated by closing over the text and taking a heavy median.
   That background is divided out, which flattens gradients and shadows.

Tesseract 5 (LSTM, `ron+eng`, the "fast" integer models, 6.5 MB in the APK) then reads the cleaned
page. It returns the text, each line's box (used to outline the cited line in the app) and the
confidence.

### Extraction, and why the model is not trusted (Rust)

`core/src/extract.rs`:

**Rules first.** Rules classify the document by its key phrases (certificat de înmatriculare,
răspundere civilă, certificat de garanție…). They find:
- dates near expiry words;
- purchase dates plus warranty periods;
- contract starts plus durations and notice periods;
- plates (with county codes) and VINs.

**Then the model.** The local model reads the pages and answers in a JSON schema, which llama.cpp
turns into a grammar, so the output is always valid JSON. For every value, the model must quote the
text it read it from. Two choices in the schema shape what a 3B model does:
- The fields are an object with one required key for each field that matters for this kind of
  document, not an array. With an array, the 3B model looped on the same field until it ran out of
  tokens. With optional keys, it stopped after the first few.
- The prompt has no concrete examples. With examples, the model copied them: a sample plate and a
  sample title showed up in real answers.

**Then the checks.** Nothing the model says is kept unless the checks pass:

- **The quote must be on the page.** The quote is located on the page it names, with approximate
  matching, because of OCR errors. If it is on another page, that page is cited instead. A quote
  that is nowhere is a hallucination, and its value is dropped. The value itself must be in the
  document too.
- **The citation is the whole line.** It shows the context ("4a. 21.01.2023 4c. DRPCIV" is an issue
  date, not an expiry).
- **The date must be in its quote.** A misread date is corrected to the nearest date in the quote.
- **A deadline is the end of a range, never its start.**
- **A quote that only says when something started or was issued** ("emis", "de la", "data vânzării")
  is replaced by the rules' deadline of that kind.
- **Computed deadlines** (a purchase date plus 24 months, a contract start plus 12 months) need both
  parts in the document, and the sum is recomputed here.
- **Only plausible deadlines.** A document keeps only the deadlines its kind can have.

### Reminders and the calendar

`core/src/reminders.rs`, `core/src/ics.rs`:

| Deadline | Reminders |
| --- | --- |
| ITP | 30 days before (plan), **14 days before: book** (stations fill up), 3 days, the day |
| RCA, CASCO | 30 days before (compare offers), 7 days (renew), the last day |
| Passport | 9 months before (many countries want 6 months left), 3 months (book: it takes weeks), 1 month, the day |
| ID card, driving licence | 60 days (book at SPCLEP / DRPCIV), 14 days, the day |
| Warranty | 30 days (check everything works while it is covered), 7 days |
| Rent contract | notice + 14 days (decide), **the last day to give notice**, the end |
| Bill | 3 days before, the due day |

Reminders that ask for an action move to the last working day before. That skips weekends and
Romania's public holidays, including the Orthodox Easter and Pentecost, which the core computes.

A deadline seen after some of its reminders have passed gets one reminder today, not a backlog. A
deadline marked done (renewed, paid) goes quiet.

On the phone, the reminders become WorkManager jobs at 09:00 on their day. Each job is named by the
reminder's stable id, so planning again replaces jobs instead of duplicating them, and a daily job
plans again.

The calendar export (RFC 5545) has an all-day event for each deadline, with an alarm for each
reminder. Booking the ITP and the last day for a notice also get events of their own. The UIDs are
stable, the lines are folded at 75 octets without splitting UTF-8 characters, and the text is
escaped.

### Search and answers

`core/src/search.rs`. The index is BM25 over passages of the OCR text, plus a "fact card" for every
deadline and field. A fact card is written out in words:
- what the deadline is, in English and the Romanian people use;
- whether it has passed;
- what the document is about (its plate, product, supplier).

The index also handles everyday language:
- Words are folded (lowercase, no diacritics; ş and ţ count as ș and ț).
- Romanian inflections are stemmed (mașina, mașinii, mașinile → masin).
- A few everyday words are expanded (buletin → carte de identitate).
- Dates are also indexed by month name, so "factura din octombrie" finds 03.10.2026.

The model answers only from the numbered passages it is given, and must say which ones it used. Any
date in its answer must be in a cited passage. Otherwise the passage that holds that date is cited,
or the answer is marked as not confirmed.

### Encryption

`core/src/crypto.rs`, `core/src/vault.rs`.

- **One seal per record.** Each record (a document's JSON, a page image) is sealed on its own with
  XChaCha20-Poly1305: a random 192-bit nonce, and the record's name as associated data. Swapping or
  renaming files makes them fail to open.
- **Atomic writes.** Files are written to a temporary file and renamed into place, so a crash never
  leaves half a record.
- **The key.** The 256-bit key comes from the core. On Android it is wrapped by an AES-GCM key that
  never leaves the Keystore. On the desktop it comes from a passphrase through Argon2id (64 MiB, 3
  passes).
- **No copies elsewhere.** The app is excluded from backups and device transfer.

## Results

Everything is measured on **synthetic documents whose truth is known**: 29 documents, 31 pages, ten
kinds, written by `tools/docket_tools/synth.py`. Each one is drawn as a page, then photographed by
code:
- on a table, at an angle;
- unevenly lit and shadowed;
- blurred, noisy and JPEG-compressed.

A third of them are flat scans. The people, plates and numbers are invented.

The evaluation runs the real `docket-ocr` and `docket` programs with the real model, served by
llama-server on an RTX 2060 (`python -m docket_tools.evaluate all`). The expected answers come from
the truth file, never from the programs.

**Page detection and OCR**: character and word error rates against the true page text, over 31
pages.

| Pipeline | CER | median CER | WER | pages under 5% CER |
| --- | --- | --- | --- | --- |
| Tesseract on the photo as it is | 34.1% | 29.1% | 43.7% | 35% |
| page found and straightened | 3.6% | 0.8% | 8.5% | 87% |
| **+ lighting flattened (Docket)** | **2.3%** | 0.9% | **7.8%** | 87% |
| + thresholded instead | 4.0% | 1.1% | 10.8% | 81% |

- The page is found in 19 of the 20 photos, with a mean corner error of 0.76% of the diagonal. The one
  miss is the second page of a contract, where a corner ran along the page's own shadow.
- Binarizing before Tesseract makes things worse, and so does Tesseract's single-column mode (not
  shown).

**Extraction**: 29 documents, from the OCR above. A deadline counts as right only with both its kind
and its exact date.

| | Kind | Deadlines | Extra deadlines | Fields | Invented values |
| --- | --- | --- | --- | --- | --- |
| Rules | 100% | 100% | 0 | 20% | 0 |
| Model, unchecked | 100% | 52% | 11 | 70% | 5 |
| **Model + checks (Docket)** | 100% | **97%** | 1 | **70%** | 2 |

On the true text (perfect OCR), the model with checks gets 100% of the deadlines and 83% of the fields.

The model alone gets half of the deadlines wrong. It takes:
- the start of "de la… până la…";
- an issue date;
- a sale date for the end of a warranty, without adding the 36 months.

It also adds deadlines that the document does not have. The checks turn that into 97–100%. The
values that remain "invented" are mostly ones the checks accepted through OCR noise (a VIN with an
O for a 0).

Two cautions about these numbers:
- The rules reach 100% on deadlines here because the same person wrote the rules and the generator.
  Real documents are more varied: that is where the model, constrained and checked, is meant to
  carry the load.
- The model brings the fields (insurer, seller, holder, product), which rules barely find.

**Questions**: 48 questions in Romanian about the 29 documents. Examples:
- "Până când e valabil RCA-ul pentru TM-87-NUA?"
- "Câtă chirie plătesc pentru apartamentul de pe Calea Victoriei?"
- "De unde am cumpărat aspirator Roborock?"

An answer is right when it holds the true date (in any written form) or value. Its citation is right
when it points at the document that states it.

| | Answer right | Cites the right document | … and page | Right and cited |
| --- | --- | --- | --- | --- |
| Search alone (the best passage is the answer) | 65% | 81% | 79% | 65% |
| **Model + checks (Docket)** | **79%** | **90%** | **88%** | **73%** |

- **Retrieval.** The document that holds the answer is the first result 81% of the time and in the
  top three 98% of the time.
- **Where the 3B model goes wrong.** It mostly mixes up similar documents: two bills from the same
  supplier, a month apart, or two policies for the same car (the expired one is marked as such). It
  also sometimes answers with the buyer when asked for the seller.
- **What the date check catches.** Wrong answers that state a date not found in their sources are
  flagged as not confirmed. Nine wrong answers still passed as confirmed, because the date or value
  they state is in a cited passage, just not the right one.

**On the phone.** All of this runs in the app:
- the instrumented tests run the native scan, the core, the encrypted vault and the reminders on the
  photos (`./gradlew connectedDebugAndroidTest`);
- the full app was tried on an x86_64 emulator, both with the model and without it.

On the emulator:
- scan and OCR take about 10 s a page;
- extraction with rules is instant;
- extraction with the 3B model took 22 minutes for one RCA, on 4 emulated cores with 8 GB.

That last number is the emulator, not a phone. No physical phone was measured, so no speed is
claimed here. For a real phone, the native library is built for ARMv8.2 with the dot-product
instructions (every phone core since 2018), which is several times faster on quantized weights. To
keep the model's work small:
- the system prompt stays computed between documents, so each new document only computes its own
  text;
- the model is asked only for the fields of the document's kind.

## Testing

```bash
cd core && cargo test                                          # Rust: crypto, vault, dates, extraction checks, reminders, calendar, search, API (43 tests)
ctest --test-dir native/build -C Release --output-on-failure   # C++: detection, perspective, lighting, JSON, OCR on a page
cd tools && python -m unittest discover -s tests -t .          # the evaluation's scoring
cd android && ./gradlew connectedDebugAndroidTest              # on a device or emulator: the whole pipeline on photos
cd tools && python -m docket_tools.evaluate all                # with the model: OCR, extraction, questions
```

CI (GitHub Actions) has four jobs:
- **core**: the Rust core on Ubuntu and Windows;
- **native**: the native code, with Tesseract built from the pinned sources;
- **tools**: the harness tests;
- **android**: builds the APK with the NDK and uploads it as an artifact.

The evaluations with the model and the emulator tests run locally.

## Layout

```text
core/                 Rust: the shared core (lib, UniFFI) and the desktop CLI (src/bin/docket.rs)
native/               C++: scan.cpp, ocr.cpp, llm.cpp (llama.cpp), jni.cpp; tools/docket_ocr.cpp; tests/
android/              Kotlin: the app (data/, scan/, reminders/, ui/) and its instrumented tests
tools/docket_tools/   Python: synth.py, photo.py, evaluate.py; tools/eval/ holds the latest results
testdata/synthetic/   the photographed documents, their clean pages and truth.json
scripts/              fetch-deps.sh, build-deps.sh, setup-model.sh, setup-llama.sh
```

## License

MIT
