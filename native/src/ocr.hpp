// Tesseract on a cleaned page: the text, and each line with its box and confidence.
#pragma once

#include <memory>
#include <opencv2/core.hpp>
#include <string>
#include <vector>

namespace tesseract {
class TessBaseAPI;
}

namespace docket {

struct OcrLine {
    std::string text;
    int x = 0, y = 0, w = 0, h = 0;
    float confidence = 0;  // 0–100
};

struct OcrPage {
    std::string text;
    std::vector<OcrLine> lines;
    float confidence = 0;  // mean over words, 0–100
};

class Ocr {
public:
    // `tessdata`: the folder with the .traineddata files; `languages`: "ron+eng".
    // `psm`: Tesseract's page segmentation mode (3: automatic, 4: one column of text).
    Ocr(const std::string& tessdata, const std::string& languages, int psm = 3);
    ~Ocr();
    Ocr(const Ocr&) = delete;
    Ocr& operator=(const Ocr&) = delete;

    // Reads an 8-bit grey (or binary) page. `dpi`: its resolution, roughly.
    OcrPage read(const cv::Mat& gray, int dpi = 200);

private:
    std::unique_ptr<tesseract::TessBaseAPI> api_;
};

}  // namespace docket
