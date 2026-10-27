// Tests of the scan pipeline on drawn images, and of OCR on a synthetic document.
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <functional>
#include <opencv2/imgcodecs.hpp>
#include <opencv2/imgproc.hpp>
#include <string>
#include <vector>

#include "json.hpp"
#include "ocr.hpp"
#include "scan.hpp"

namespace {

int failures = 0;

void check(bool ok, const std::string& what) {
    std::printf("%s %s\n", ok ? "ok  " : "FAIL", what.c_str());
    if (!ok) ++failures;
}

double max_corner_error(const std::array<cv::Point2f, 4>& a, const std::array<cv::Point2f, 4>& b) {
    double e = 0;
    for (int i = 0; i < 4; ++i) e = std::max(e, static_cast<double>(std::hypot(a[i].x - b[i].x, a[i].y - b[i].y)));
    return e;
}

// A white page with text-like bars, at a perspective, on a dark textured table.
cv::Mat photo_of_page(const std::array<cv::Point2f, 4>& corners, cv::Size size) {
    cv::Mat page(1400, 1000, CV_8UC3, cv::Scalar(245, 245, 240));
    for (int y = 120; y < 1300; y += 45) cv::rectangle(page, {100, y}, {100 + (y * 37) % 700 + 100, y + 18}, cv::Scalar(30, 30, 30), cv::FILLED);
    std::array<cv::Point2f, 4> src = {cv::Point2f(0, 0), cv::Point2f(1000, 0), cv::Point2f(1000, 1400), cv::Point2f(0, 1400)};
    cv::Mat m = cv::getPerspectiveTransform(src.data(), corners.data());
    cv::Mat table(size, CV_8UC3);
    cv::randu(table, cv::Scalar(40, 30, 20), cv::Scalar(90, 70, 50));
    cv::GaussianBlur(table, table, {7, 7}, 0);
    cv::Mat out = table.clone();
    cv::warpPerspective(page, out, m, size, cv::INTER_LINEAR, cv::BORDER_TRANSPARENT);
    return out;
}

void test_order_corners() {
    std::array<cv::Point2f, 4> shuffled = {cv::Point2f(900, 1200), cv::Point2f(100, 80), cv::Point2f(80, 1150), cv::Point2f(950, 60)};
    auto o = docket::order_corners(shuffled);
    check(o[0] == cv::Point2f(100, 80) && o[1] == cv::Point2f(950, 60) && o[2] == cv::Point2f(900, 1200) && o[3] == cv::Point2f(80, 1150),
          "corners are ordered clockwise from the top left");
}

void test_detects_a_tilted_page() {
    std::array<cv::Point2f, 4> truth = {cv::Point2f(260, 230), cv::Point2f(1310, 300), cv::Point2f(1230, 1780), cv::Point2f(180, 1700)};
    cv::Mat photo = photo_of_page(truth, {1500, 2000});
    auto q = docket::detect_page(photo);
    double e = max_corner_error(q.corners, truth);
    check(q.found && e < 12, "a tilted page is found (corner error " + std::to_string(e) + " px)");
}

void test_no_page_is_no_page() {
    cv::Mat noise(1200, 900, CV_8UC3);
    cv::randu(noise, cv::Scalar(0, 0, 0), cv::Scalar(255, 255, 255));
    auto q = docket::detect_page(noise);
    check(!q.found && q.corners[2] == cv::Point2f(900, 1200), "noise has no page: the whole photo is used");
}

void test_warp_is_upright_and_sized() {
    std::array<cv::Point2f, 4> truth = {cv::Point2f(260, 230), cv::Point2f(1310, 300), cv::Point2f(1230, 1780), cv::Point2f(180, 1700)};
    cv::Mat photo = photo_of_page(truth, {1500, 2000});
    cv::Mat page = docket::warp(photo, truth, 2000);
    double ratio = static_cast<double>(page.rows) / page.cols;
    check(page.rows == 2000 && std::fabs(ratio - 1.4) < 0.05, "the warped page is upright with its proportions (" + std::to_string(ratio) + ")");
    // The page's corners are white paper after the warp, not table.
    cv::Mat gray;
    cv::cvtColor(page, gray, cv::COLOR_BGR2GRAY);
    check(gray.at<uchar>(10, 10) > 200 && gray.at<uchar>(page.rows - 10, page.cols - 10) > 200, "the warp keeps paper, not table");
}

void test_flatten_light() {
    // Paper lit from the left: 250 down to 120; text bars at 40% of the local light.
    cv::Mat gray(800, 600, CV_8UC1);
    for (int y = 0; y < gray.rows; ++y)
        for (int x = 0; x < gray.cols; ++x) {
            double light = 250 - 130.0 * x / gray.cols;
            bool ink = (y / 20) % 3 == 0 && x % 50 < 30;
            gray.at<uchar>(y, x) = cv::saturate_cast<uchar>(ink ? light * 0.4 : light);
        }
    cv::Mat flat = docket::flatten_light(gray);
    // Paper on the dark side ends up as white as on the bright side.
    double left = flat.at<uchar>(30, 40), right = flat.at<uchar>(30, 560);
    double ink = flat.at<uchar>(5, 565);
    check(std::fabs(left - right) < 25 && right > 200, "flattening evens the paper (" + std::to_string(left) + " vs " + std::to_string(right) + ")");
    check(ink < 140, "and keeps ink dark (" + std::to_string(ink) + ")");
}

void test_json() {
    check(docket::json_escape("a\"b\\c\nd\x01") == "a\\\"b\\\\c\\nd\\u0001", "JSON escaping");
    check(docket::json_escape("Poliță") == "Poliță", "UTF-8 passes through");
}

void test_ocr_reads_a_document() {
    std::string page = std::string(DOCKET_TESTDATA) + "/clean/rca-1-p1.png";
    std::string tessdata = DOCKET_TESSDATA;
    if (!std::filesystem::exists(page) || !std::filesystem::exists(tessdata + "/ron.traineddata")) {
        std::printf("skip OCR test: no test data or tessdata (scripts/fetch-deps.sh)\n");
        return;
    }
    cv::Mat img = cv::imread(page, cv::IMREAD_COLOR);
    docket::ScanOptions opt;
    opt.detect = false;
    auto s = docket::scan(img, opt);
    docket::Ocr ocr(tessdata, "ron+eng");
    auto r = ocr.read(s.for_ocr);
    check(r.text.find("ASIGURARE RCA") != std::string::npos, "OCR reads the title");
    check(r.text.find("RĂSPUNDERE CIVILĂ") != std::string::npos, "with its diacritics");
    check(r.text.find("Perioada de valabilitate") != std::string::npos, "and the validity line");
    check(r.lines.size() > 15 && r.confidence > 80, "with line boxes and confidence " + std::to_string(r.confidence));
}

}  // namespace

int main() {
    test_order_corners();
    test_detects_a_tilted_page();
    test_no_page_is_no_page();
    test_warp_is_upright_and_sized();
    test_flatten_light();
    test_json();
    test_ocr_reads_a_document();
    std::printf("%d failure(s)\n", failures);
    return failures == 0 ? 0 : 1;
}
