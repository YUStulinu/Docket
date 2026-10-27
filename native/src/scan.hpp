// Finding the page in a photo and making it look scanned.
//
// 1. Detection: the page is the largest convex quadrilateral that stands out from the table, by its
//    edges (Canny) or by being brighter than its surroundings (Otsu). Each candidate is scored by
//    its size and how rectangular it is; the best one wins.
// 2. Perspective: the four corners are mapped onto an upright rectangle of the page's own
//    proportions, at a resolution where text is about 200 DPI.
// 3. Light: the page's background (the image with the text closed over) is divided out, which
//    flattens gradients and shadows; what is left is dark text on white.
#pragma once

#include <array>
#include <opencv2/core.hpp>

namespace docket {

enum class Cleanup {
    None,    // the warped colour page as it is
    Gray,    // grey only
    Flat,    // grey with the lighting flattened (the default for OCR)
    Binary,  // flattened, then thresholded (Sauvola-like local threshold)
};

struct ScanOptions {
    bool detect = true;           // false: the photo is already the page (a flat scan)
    Cleanup cleanup = Cleanup::Flat;
    int target_long_side = 2300;  // pixels of the corrected page's long side
};

struct ScanResult {
    cv::Mat page;     // the corrected page, for display and storage (BGR)
    cv::Mat for_ocr;  // what Tesseract reads (8-bit grey)
    std::array<cv::Point2f, 4> corners{};  // in the photo: top left, top right, bottom right, bottom left
    bool found = false;  // a page was detected (otherwise the whole photo is used)
    double score = 0;
};

// The four corners of the page in `bgr`, ordered; found=false if there is no convincing page.
struct Quad {
    std::array<cv::Point2f, 4> corners{};
    bool found = false;
    double score = 0;
};
Quad detect_page(const cv::Mat& bgr);

// Orders four points: top left, top right, bottom right, bottom left.
std::array<cv::Point2f, 4> order_corners(std::array<cv::Point2f, 4> pts);

// Warps the quad to an upright page.
cv::Mat warp(const cv::Mat& bgr, const std::array<cv::Point2f, 4>& corners, int target_long_side);

// Divides out the lighting of a grey page.
cv::Mat flatten_light(const cv::Mat& gray);

// A local threshold of a flattened page.
cv::Mat binarize(const cv::Mat& flat);

ScanResult scan(const cv::Mat& bgr, const ScanOptions& options = {});

}  // namespace docket
