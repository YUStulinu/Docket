#include "scan.hpp"

#include <algorithm>
#include <cmath>
#include <opencv2/imgproc.hpp>
#include <vector>

namespace docket {

namespace {

constexpr int kDetectSide = 800;  // detection runs on a smaller copy

double angle_cos(cv::Point2f a, cv::Point2f b, cv::Point2f c) {
    // The cosine of the angle at b.
    cv::Point2f u = a - b, v = c - b;
    return (u.x * v.x + u.y * v.y) / (std::sqrt((u.x * u.x + u.y * u.y) * (v.x * v.x + v.y * v.y)) + 1e-9);
}

// A contour reduced to four corners, if it can be.
bool to_quad(const std::vector<cv::Point>& contour, std::array<cv::Point2f, 4>& out) {
    std::vector<cv::Point> hull;
    cv::convexHull(contour, hull);
    double perimeter = cv::arcLength(hull, true);
    for (double eps = 0.01; eps <= 0.08; eps += 0.01) {
        std::vector<cv::Point> approx;
        cv::approxPolyDP(hull, approx, eps * perimeter, true);
        if (approx.size() == 4 && cv::isContourConvex(approx)) {
            for (int i = 0; i < 4; ++i) out[i] = cv::Point2f(static_cast<float>(approx[i].x), static_cast<float>(approx[i].y));
            return true;
        }
        if (approx.size() < 4) break;
    }
    return false;
}

// Larger and more rectangular is better; tiny or skewed shapes are not pages.
double quad_score(const std::array<cv::Point2f, 4>& q, double image_area) {
    std::vector<cv::Point2f> poly(q.begin(), q.end());
    double area = std::fabs(cv::contourArea(poly));
    double frac = area / image_area;
    if (frac < 0.15 || frac > 0.995) return 0;
    double worst = 0;
    for (int i = 0; i < 4; ++i) worst = std::max(worst, std::fabs(angle_cos(q[(i + 3) % 4], q[i], q[(i + 1) % 4])));
    if (worst > 0.35) return 0;  // corners further than ~70° or ~110° from square
    return frac * (1.0 - worst);
}

void candidates_from(const cv::Mat& mask, double image_area, Quad& best) {
    std::vector<std::vector<cv::Point>> contours;
    cv::findContours(mask, contours, cv::RETR_LIST, cv::CHAIN_APPROX_SIMPLE);
    for (const auto& c : contours) {
        if (cv::contourArea(c) < 0.1 * image_area) continue;
        std::array<cv::Point2f, 4> q;
        if (!to_quad(c, q)) continue;
        double s = quad_score(q, image_area);
        if (s > best.score) {
            best.score = s;
            best.corners = q;
            best.found = true;
        }
    }
}

}  // namespace

std::array<cv::Point2f, 4> order_corners(std::array<cv::Point2f, 4> p) {
    std::array<cv::Point2f, 4> o;
    auto sum = [](cv::Point2f a) { return a.x + a.y; };
    auto diff = [](cv::Point2f a) { return a.y - a.x; };
    o[0] = *std::min_element(p.begin(), p.end(), [&](auto a, auto b) { return sum(a) < sum(b); });
    o[2] = *std::max_element(p.begin(), p.end(), [&](auto a, auto b) { return sum(a) < sum(b); });
    o[1] = *std::min_element(p.begin(), p.end(), [&](auto a, auto b) { return diff(a) < diff(b); });
    o[3] = *std::max_element(p.begin(), p.end(), [&](auto a, auto b) { return diff(a) < diff(b); });
    return o;
}

Quad detect_page(const cv::Mat& bgr) {
    double s = static_cast<double>(kDetectSide) / std::max(bgr.cols, bgr.rows);
    cv::Mat small, gray;
    cv::resize(bgr, small, cv::Size(), s, s, cv::INTER_AREA);
    cv::cvtColor(small, gray, cv::COLOR_BGR2GRAY);
    cv::GaussianBlur(gray, gray, cv::Size(5, 5), 0);
    double image_area = static_cast<double>(gray.rows) * gray.cols;
    Quad best;

    // By edges: thresholds from the median brightness.
    std::vector<uchar> v(gray.begin<uchar>(), gray.end<uchar>());
    std::nth_element(v.begin(), v.begin() + v.size() / 2, v.end());
    double med = v[v.size() / 2];
    cv::Mat edges;
    cv::Canny(gray, edges, std::max(10.0, 0.66 * med), std::min(255.0, 1.33 * med));
    cv::dilate(edges, edges, cv::getStructuringElement(cv::MORPH_RECT, cv::Size(3, 3)));
    candidates_from(edges, image_area, best);

    // By brightness: paper is lighter than the table.
    cv::Mat bright;
    cv::threshold(gray, bright, 0, 255, cv::THRESH_BINARY | cv::THRESH_OTSU);
    cv::morphologyEx(bright, bright, cv::MORPH_CLOSE, cv::getStructuringElement(cv::MORPH_RECT, cv::Size(15, 15)));
    candidates_from(bright, image_area, best);

    if (best.found) {
        for (auto& c : best.corners) c *= static_cast<float>(1.0 / s);
        best.corners = order_corners(best.corners);
    } else {
        float w = static_cast<float>(bgr.cols), h = static_cast<float>(bgr.rows);
        best.corners = {cv::Point2f(0, 0), cv::Point2f(w, 0), cv::Point2f(w, h), cv::Point2f(0, h)};
    }
    return best;
}

cv::Mat warp(const cv::Mat& bgr, const std::array<cv::Point2f, 4>& c, int target_long_side) {
    auto dist = [](cv::Point2f a, cv::Point2f b) { return std::hypot(a.x - b.x, a.y - b.y); };
    double w = std::max(dist(c[0], c[1]), dist(c[3], c[2]));
    double h = std::max(dist(c[0], c[3]), dist(c[1], c[2]));
    double k = target_long_side / std::max(w, h);
    int W = std::max(16, static_cast<int>(std::lround(w * k)));
    int H = std::max(16, static_cast<int>(std::lround(h * k)));
    std::array<cv::Point2f, 4> dst = {cv::Point2f(0, 0), cv::Point2f(static_cast<float>(W), 0),
                                      cv::Point2f(static_cast<float>(W), static_cast<float>(H)), cv::Point2f(0, static_cast<float>(H))};
    cv::Mat m = cv::getPerspectiveTransform(c.data(), dst.data());
    cv::Mat out;
    cv::warpPerspective(bgr, out, m, cv::Size(W, H), cv::INTER_CUBIC, cv::BORDER_REPLICATE);
    return out;
}

cv::Mat flatten_light(const cv::Mat& gray) {
    // The background: text closed over (dilation removes dark strokes), then smoothed. Work on a
    // smaller copy: the light changes slowly.
    cv::Mat small, bg;
    double s = 0.25;
    cv::resize(gray, small, cv::Size(), s, s, cv::INTER_AREA);
    cv::dilate(small, bg, cv::getStructuringElement(cv::MORPH_ELLIPSE, cv::Size(9, 9)));
    cv::medianBlur(bg, bg, 21);
    cv::resize(bg, bg, gray.size(), 0, 0, cv::INTER_LINEAR);
    cv::Mat out;
    cv::divide(gray, bg, out, 255.0);
    // Stretch: the paper to white, the darkest ink to black.
    double lo, hi;
    cv::minMaxLoc(out, &lo, &hi);
    std::vector<uchar> v(out.begin<uchar>(), out.end<uchar>());
    std::nth_element(v.begin(), v.begin() + v.size() / 200, v.end());
    lo = v[v.size() / 200];
    if (hi - lo > 10) out.convertTo(out, CV_8U, 255.0 / (hi - lo), -lo * 255.0 / (hi - lo));
    return out;
}

cv::Mat binarize(const cv::Mat& flat) {
    cv::Mat out;
    int block = std::max(15, (std::min(flat.cols, flat.rows) / 40) | 1);
    cv::adaptiveThreshold(flat, out, 255, cv::ADAPTIVE_THRESH_GAUSSIAN_C, cv::THRESH_BINARY, block, 12);
    return out;
}

ScanResult scan(const cv::Mat& bgr, const ScanOptions& opt) {
    ScanResult r;
    if (opt.detect) {
        Quad q = detect_page(bgr);
        r.corners = q.corners;
        r.found = q.found;
        r.score = q.score;
    } else {
        float w = static_cast<float>(bgr.cols), h = static_cast<float>(bgr.rows);
        r.corners = {cv::Point2f(0, 0), cv::Point2f(w, 0), cv::Point2f(w, h), cv::Point2f(0, h)};
    }
    r.page = warp(bgr, r.corners, opt.target_long_side);
    if (r.found) {
        // A corner found a few pixels outside the paper leaves a sliver of table at the edges,
        // which OCR reads as "|". Paper margins are wider than what is cut.
        int mx = r.page.cols * 3 / 200, my = r.page.rows * 3 / 200;
        r.page = r.page(cv::Rect(mx, my, r.page.cols - 2 * mx, r.page.rows - 2 * my)).clone();
    }
    cv::Mat gray;
    cv::cvtColor(r.page, gray, cv::COLOR_BGR2GRAY);
    switch (opt.cleanup) {
        case Cleanup::None:
        case Cleanup::Gray: r.for_ocr = gray; break;
        case Cleanup::Flat: r.for_ocr = flatten_light(gray); break;
        case Cleanup::Binary: r.for_ocr = binarize(flatten_light(gray)); break;
    }
    return r;
}

}  // namespace docket
