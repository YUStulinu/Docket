// The OCR result as the JSON the Rust core reads (see core/src/bin/docket.rs, OcrPage).
#pragma once

#include <array>
#include <opencv2/core.hpp>
#include <string>

#include "ocr.hpp"

namespace docket {

std::string json_escape(const std::string& s);

struct PageJson {
    std::string image;  // file name of the corrected page, or empty
    OcrPage ocr;
    int width = 0, height = 0;
    std::array<cv::Point2f, 4> corners{};
    bool found = false;
    double ms_scan = 0, ms_ocr = 0;
};

std::string page_json(const PageJson& p);

}  // namespace docket
