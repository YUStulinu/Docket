// The JNI bridge for the Android app (io.github.yustulinu.docket.scan.NativeScan).
#include <jni.h>

#include <android/log.h>
#include <memory>
#include <mutex>
#include <opencv2/imgcodecs.hpp>
#include <opencv2/imgproc.hpp>
#include <string>
#include <vector>

#include "json.hpp"
#include "llm.hpp"
#include "ocr.hpp"
#include "scan.hpp"

namespace {

std::mutex ocr_mutex;
std::unique_ptr<docket::Ocr> ocr;
std::string ocr_path;

std::string str(JNIEnv* env, jstring s) {
    if (!s) return {};
    const char* c = env->GetStringUTFChars(s, nullptr);
    std::string out(c);
    env->ReleaseStringUTFChars(s, c);
    return out;
}

void throw_java(JNIEnv* env, const std::string& what) {
    __android_log_print(ANDROID_LOG_ERROR, "docket", "%s", what.c_str());
    env->ThrowNew(env->FindClass("java/lang/RuntimeException"), what.c_str());
}

jstring utf8(JNIEnv* env, const std::string& s) {
    // NewStringUTF wants modified UTF-8; go through a Java String(byte[], "UTF-8") instead, which
    // takes any text (emoji in OCR output included).
    jbyteArray bytes = env->NewByteArray(static_cast<jsize>(s.size()));
    env->SetByteArrayRegion(bytes, 0, static_cast<jsize>(s.size()), reinterpret_cast<const jbyte*>(s.data()));
    jclass cls = env->FindClass("java/lang/String");
    jmethodID ctor = env->GetMethodID(cls, "<init>", "([BLjava/lang/String;)V");
    return static_cast<jstring>(env->NewObject(cls, ctor, bytes, env->NewStringUTF("UTF-8")));
}

}  // namespace

extern "C" {

// [json, jpeg]: the OCR of the page (as docket-ocr writes it) and the corrected page.
JNIEXPORT jobjectArray JNICALL Java_io_github_yustulinu_docket_scan_NativeScan_scan(JNIEnv* env, jobject, jbyteArray photo, jboolean detect,
                                                                               jstring tessdata) {
    try {
        jsize n = env->GetArrayLength(photo);
        std::vector<uchar> bytes(n);
        env->GetByteArrayRegion(photo, 0, n, reinterpret_cast<jbyte*>(bytes.data()));
        cv::Mat img = cv::imdecode(bytes, cv::IMREAD_COLOR);  // applies the EXIF orientation
        if (img.empty()) {
            throw_java(env, "not an image");
            return nullptr;
        }
        docket::ScanOptions opt;
        opt.detect = detect;
        docket::ScanResult s = docket::scan(img, opt);
        docket::PageJson p;
        {
            std::lock_guard<std::mutex> lock(ocr_mutex);
            std::string path = str(env, tessdata);
            if (!ocr || ocr_path != path) {
                ocr = std::make_unique<docket::Ocr>(path, "ron+eng");
                ocr_path = path;
            }
            p.ocr = ocr->read(s.for_ocr);
        }
        p.width = s.page.cols;
        p.height = s.page.rows;
        p.corners = s.corners;
        p.found = s.found;
        std::vector<uchar> jpeg;
        cv::imencode(".jpg", s.page, jpeg, {cv::IMWRITE_JPEG_QUALITY, 85});
        jobjectArray out = env->NewObjectArray(2, env->FindClass("java/lang/Object"), nullptr);
        env->SetObjectArrayElement(out, 0, utf8(env, docket::page_json(p)));
        jbyteArray j = env->NewByteArray(static_cast<jsize>(jpeg.size()));
        env->SetByteArrayRegion(j, 0, static_cast<jsize>(jpeg.size()), reinterpret_cast<const jbyte*>(jpeg.data()));
        env->SetObjectArrayElement(out, 1, j);
        return out;
    } catch (const std::exception& e) {
        throw_java(env, e.what());
        return nullptr;
    }
}

// The page in a camera frame (its luminance plane), for the live outline: x0,y0..x3,y3 as fractions
// of the frame, then the score (0: no page).
JNIEXPORT jfloatArray JNICALL Java_io_github_yustulinu_docket_scan_NativeScan_detectQuad(JNIEnv* env, jobject, jbyteArray luma, jint width,
                                                                                    jint height, jint stride) {
    std::vector<uchar> bytes(env->GetArrayLength(luma));
    env->GetByteArrayRegion(luma, 0, static_cast<jsize>(bytes.size()), reinterpret_cast<jbyte*>(bytes.data()));
    cv::Mat y(height, width, CV_8UC1, bytes.data(), static_cast<size_t>(stride));
    cv::Mat bgr;
    cv::cvtColor(y, bgr, cv::COLOR_GRAY2BGR);
    docket::Quad q = docket::detect_page(bgr);
    float out[9];
    for (int i = 0; i < 4; ++i) {
        out[2 * i] = q.corners[i].x / static_cast<float>(width);
        out[2 * i + 1] = q.corners[i].y / static_cast<float>(height);
    }
    out[8] = q.found ? static_cast<float>(q.score) : 0.0f;
    jfloatArray r = env->NewFloatArray(9);
    env->SetFloatArrayRegion(r, 0, 9, out);
    return r;
}

JNIEXPORT jlong JNICALL Java_io_github_yustulinu_docket_scan_NativeScan_llmOpen(JNIEnv* env, jobject, jstring path, jint context, jint threads) {
    try {
        return reinterpret_cast<jlong>(new docket::Llm(str(env, path), context, threads));
    } catch (const std::exception& e) {
        throw_java(env, e.what());
        return 0;
    }
}

JNIEXPORT jstring JNICALL Java_io_github_yustulinu_docket_scan_NativeScan_llmComplete(JNIEnv* env, jobject, jlong handle, jstring system,
                                                                                 jstring user, jstring schema, jint max_tokens) {
    try {
        auto* llm = reinterpret_cast<docket::Llm*>(handle);
        return utf8(env, llm->complete(str(env, system), str(env, user), str(env, schema), max_tokens));
    } catch (const std::exception& e) {
        throw_java(env, e.what());
        return nullptr;
    }
}

JNIEXPORT jstring JNICALL Java_io_github_yustulinu_docket_scan_NativeScan_llmName(JNIEnv* env, jobject, jlong handle) {
    return utf8(env, reinterpret_cast<docket::Llm*>(handle)->name());
}

JNIEXPORT void JNICALL Java_io_github_yustulinu_docket_scan_NativeScan_llmClose(JNIEnv*, jobject, jlong handle) {
    delete reinterpret_cast<docket::Llm*>(handle);
}

}  // extern "C"
