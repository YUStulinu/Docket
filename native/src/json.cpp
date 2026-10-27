#include "json.hpp"

#include <cstdio>
#include <sstream>

namespace docket {

std::string json_escape(const std::string& s) {
    std::string out;
    out.reserve(s.size() + 2);
    for (unsigned char c : s) {
        switch (c) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default:
                if (c < 0x20) {
                    char buf[8];
                    std::snprintf(buf, sizeof buf, "\\u%04x", c);
                    out += buf;
                } else {
                    out += static_cast<char>(c);  // UTF-8 passes through
                }
        }
    }
    return out;
}

std::string page_json(const PageJson& p) {
    std::ostringstream o;
    o.precision(6);
    o << "{";
    if (!p.image.empty()) o << "\"image\":\"" << json_escape(p.image) << "\",";
    o << "\"text\":\"" << json_escape(p.ocr.text) << "\",";
    o << "\"width\":" << p.width << ",\"height\":" << p.height << ",";
    o << "\"confidence\":" << p.ocr.confidence << ",";
    o << "\"found\":" << (p.found ? "true" : "false") << ",";
    o << "\"corners\":[";
    for (int i = 0; i < 4; ++i) o << (i ? "," : "") << "[" << p.corners[i].x << "," << p.corners[i].y << "]";
    o << "],\"ms_scan\":" << p.ms_scan << ",\"ms_ocr\":" << p.ms_ocr << ",\"lines\":[";
    for (size_t i = 0; i < p.ocr.lines.size(); ++i) {
        const auto& l = p.ocr.lines[i];
        o << (i ? "," : "") << "{\"text\":\"" << json_escape(l.text) << "\",\"x\":" << l.x << ",\"y\":" << l.y << ",\"w\":" << l.w
          << ",\"h\":" << l.h << ",\"confidence\":" << l.confidence << "}";
    }
    o << "]}";
    return o.str();
}

}  // namespace docket
