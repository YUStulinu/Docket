#include "ocr.hpp"

#include <leptonica/allheaders.h>
#include <stdexcept>
#include <tesseract/baseapi.h>
#include <tesseract/resultiterator.h>

namespace docket {

Ocr::Ocr(const std::string& tessdata, const std::string& languages, int psm) : api_(std::make_unique<tesseract::TessBaseAPI>()) {
    // Leptonica is built without image codecs (OpenCV decodes); its complaints about them are noise.
    setMsgSeverity(L_SEVERITY_NONE);
    if (api_->Init(tessdata.c_str(), languages.c_str(), tesseract::OEM_LSTM_ONLY) != 0) {
        throw std::runtime_error("Tesseract could not load " + languages + " from " + tessdata);
    }
    api_->SetPageSegMode(static_cast<tesseract::PageSegMode>(psm));
    // Keep the spacing of forms ("Data ITP:   12.04.2025") and do not invent dictionary words for
    // numbers and codes.
    api_->SetVariable("preserve_interword_spaces", "1");
    api_->SetVariable("user_defined_dpi", "200");
}

Ocr::~Ocr() {
    if (api_) api_->End();
}

OcrPage Ocr::read(const cv::Mat& gray, int dpi) {
    if (gray.type() != CV_8UC1) throw std::invalid_argument("OCR needs an 8-bit grey image");
    cv::Mat img = gray.isContinuous() ? gray : gray.clone();
    api_->SetImage(img.data, img.cols, img.rows, 1, static_cast<int>(img.step));
    api_->SetSourceResolution(dpi);
    if (api_->Recognize(nullptr) != 0) throw std::runtime_error("Tesseract failed on the page");

    OcrPage page;
    std::unique_ptr<char[]> text(api_->GetUTF8Text());
    page.text = text ? text.get() : "";
    page.confidence = static_cast<float>(api_->MeanTextConf());

    std::unique_ptr<tesseract::ResultIterator> it(api_->GetIterator());
    if (it) {
        do {
            std::unique_ptr<char[]> line(it->GetUTF8Text(tesseract::RIL_TEXTLINE));
            if (!line) continue;
            OcrLine l;
            l.text = line.get();
            while (!l.text.empty() && (l.text.back() == '\n' || l.text.back() == ' ')) l.text.pop_back();
            if (l.text.empty()) continue;
            int x1, y1, x2, y2;
            it->BoundingBox(tesseract::RIL_TEXTLINE, &x1, &y1, &x2, &y2);
            l.x = x1;
            l.y = y1;
            l.w = x2 - x1;
            l.h = y2 - y1;
            l.confidence = it->Confidence(tesseract::RIL_TEXTLINE);
            page.lines.push_back(std::move(l));
        } while (it->Next(tesseract::RIL_TEXTLINE));
    }
    api_->Clear();
    return page;
}

}  // namespace docket
