// docket-ocr: photos of one document in, the corrected pages and their OCR (JSON) out — the same
// pipeline the app runs on the phone.
//
//   docket-ocr --tessdata DIR [--lang ron+eng] [--cleanup flat|gray|binary] [--no-detect] [--psm 3|4]
//              --out DIR --name NAME photo1.jpg [photo2.jpg ...]
//
// Writes DIR/NAME.json ({"pages": [...]}, as `docket add` reads it) and DIR/NAME-pN.jpg.

#include <chrono>
#include <cstdlib>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <opencv2/imgcodecs.hpp>
#include <string>
#include <vector>

#include "json.hpp"
#include "ocr.hpp"
#include "scan.hpp"

namespace fs = std::filesystem;
using Clock = std::chrono::steady_clock;

static int usage() {
    std::cerr << "usage: docket-ocr --tessdata DIR [--lang ron+eng] [--cleanup flat|gray|binary] [--no-detect] --out DIR --name NAME photo...\n";
    return 2;
}

int main(int argc, char** argv) {
    std::string tessdata, lang = "ron+eng", out, name, cleanup = "flat";
    bool detect = true;
    int psm = 3;
    std::vector<std::string> photos;
    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        auto next = [&]() -> std::string { return i + 1 < argc ? argv[++i] : ""; };
        if (a == "--tessdata") tessdata = next();
        else if (a == "--lang") lang = next();
        else if (a == "--out") out = next();
        else if (a == "--name") name = next();
        else if (a == "--cleanup") cleanup = next();
        else if (a == "--no-detect") detect = false;
        else if (a == "--psm") psm = std::atoi(next().c_str());
        else if (!a.empty() && a[0] == '-') return usage();
        else photos.push_back(a);
    }
    if (tessdata.empty() || out.empty() || name.empty() || photos.empty()) return usage();

    docket::ScanOptions opt;
    opt.detect = detect;
    opt.cleanup = cleanup == "gray" ? docket::Cleanup::Gray : cleanup == "binary" ? docket::Cleanup::Binary
                : cleanup == "none" ? docket::Cleanup::None : docket::Cleanup::Flat;
    try {
        docket::Ocr ocr(tessdata, lang, psm);
        fs::create_directories(out);
        std::string json = "{\"pages\":[";
        for (size_t n = 0; n < photos.size(); ++n) {
            cv::Mat photo = cv::imread(photos[n], cv::IMREAD_COLOR);
            if (photo.empty()) {
                std::cerr << "cannot read " << photos[n] << "\n";
                return 1;
            }
            auto t0 = Clock::now();
            docket::ScanResult s = docket::scan(photo, opt);
            auto t1 = Clock::now();
            docket::PageJson p;
            p.ocr = ocr.read(s.for_ocr);
            auto t2 = Clock::now();
            p.image = name + "-p" + std::to_string(n + 1) + ".jpg";
            cv::imwrite((fs::path(out) / p.image).string(), s.page, {cv::IMWRITE_JPEG_QUALITY, 85});
            p.width = s.page.cols;
            p.height = s.page.rows;
            p.corners = s.corners;
            p.found = s.found;
            p.ms_scan = std::chrono::duration<double, std::milli>(t1 - t0).count();
            p.ms_ocr = std::chrono::duration<double, std::milli>(t2 - t1).count();
            json += (n ? "," : "") + docket::page_json(p);
        }
        json += "]}";
        std::ofstream(fs::path(out) / (name + ".json"), std::ios::binary) << json;
    } catch (const std::exception& e) {
        std::cerr << "error: " << e.what() << "\n";
        return 1;
    }
    return 0;
}
