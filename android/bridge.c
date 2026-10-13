/* Native Android host boundary. All callbacks run on the Java UI thread.
 * Rust owns widgets, layout, fonts, state and pixels; Java owns the Android View/IME.
 */
#include <jni.h>
#include <android/bitmap.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <time.h>

extern uintptr_t jfgui_buffer(uint32_t length);
extern int32_t jfgui_start(int32_t width, int32_t height, int32_t dpi, uint32_t args_len);
extern void jfgui_tick(uint32_t time_ms);
extern void jfgui_input(uint32_t time_ms);
extern uint32_t jfgui_event(uint32_t type, uint32_t button, uint32_t mods, uint32_t window,
                             int32_t x, int32_t y, uint32_t data);
extern uint32_t jfgui_text(uint32_t length);
extern uint32_t jfgui_complete(uint32_t id, int32_t status);
extern uint32_t jfgui_frame_delay(void);
extern uint32_t jfgui_editor_snapshot(void);
extern uintptr_t jfgui_editor_data(void);
extern uint32_t jfgui_editor_replace(uint32_t id, uint32_t length, uint32_t anchor, uint32_t caret);

static JNIEnv *target_env;
static jobject target_bitmap;
static AndroidBitmapInfo target_info;
static int started, last_width, last_height, last_dpi, ime_visible, presented;
static uint64_t epoch_ms;

double jfgui_host_time_us(void) {
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return 0;
    return (double)now.tv_sec * 1000000.0 + (double)now.tv_nsec / 1000.0;
}
static uint32_t time_ms(void) {
    uint64_t now = (uint64_t)jfgui_host_time_us() / 1000;
    if (!epoch_ms) epoch_ms = now;
    return (uint32_t)(now - epoch_ms);
}

void jfgui_host_present(const uint8_t *source, size_t length, int32_t width, int32_t height,
                         int32_t stride, const int32_t (*rects)[4], size_t count) {
    if (!target_bitmap || !source || !rects || !count || count > 128
        || width <= 0 || height <= 0 || width > 4096 || height > 4096
        || stride < width * 4 || (size_t)stride * (size_t)height > length
        || target_info.width != (uint32_t)width || target_info.height != (uint32_t)height
        || target_info.stride < (uint32_t)width * 4) return;
    uint8_t *pixels = NULL;
    // Locking even an unchanged Bitmap changes its generation and triggers a GPU upload.
    // Only touch it when Rust reports damage, and only copy those rectangles.
    if (AndroidBitmap_lockPixels(target_env, target_bitmap, (void **)&pixels) != ANDROID_BITMAP_RESULT_SUCCESS) {
        presented = -3;
        return;
    }
    for (size_t i = 0; i < count; ++i) {
        int64_t right = (int64_t)rects[i][0] + rects[i][2];
        int64_t bottom = (int64_t)rects[i][1] + rects[i][3];
        int left = rects[i][0] < 0 ? 0 : rects[i][0];
        int top = rects[i][1] < 0 ? 0 : rects[i][1];
        int end_x = right > width ? width : (int)right;
        int end_y = bottom > height ? height : (int)bottom;
        if (end_x <= left || end_y <= top) continue;
        for (int y = top; y < end_y; ++y) {
            const uint8_t *s = source + (size_t)y * (size_t)stride + (size_t)left * 4;
            uint8_t *d = pixels + (size_t)y * target_info.stride + (size_t)left * 4;
            for (int x = left; x < end_x; ++x) {
                d[0] = s[2]; d[1] = s[1]; d[2] = s[0]; d[3] = 255;
                d += 4; s += 4;
            }
        }
    }
    AndroidBitmap_unlockPixels(target_env, target_bitmap);
    presented = 1;
}
void jfgui_host_request(uint32_t kind, uint32_t id, const uint8_t *bytes, size_t length) {
    if (kind == 6 && bytes && length == 12) {
        int32_t height;
        memcpy(&height, bytes + 8, sizeof(height));
        ime_visible = height > 0;
    }
    /* This prototype offers virtual files and in-app rich clipboard only.
     * Resolve every unsupported asynchronous service instead of leaving it pending. */
    if (id) (void)jfgui_complete(id, -3);
}

JNIEXPORT jint JNICALL Java_org_jfgui_gallery_GalleryActivity_nativeRender(
        JNIEnv *env, jclass cls, jobject bitmap, jint dpi, jboolean new_surface) {
    (void)cls;
    if (AndroidBitmap_getInfo(env, bitmap, &target_info) != ANDROID_BITMAP_RESULT_SUCCESS
        || target_info.format != ANDROID_BITMAP_FORMAT_RGBA_8888
        || target_info.width == 0 || target_info.height == 0
        || target_info.width > 4096 || target_info.height > 4096) return -2;
    target_env = env;
    target_bitmap = bitmap;
    presented = 0;
    int result = 0;
    if (!started) {
        (void)jfgui_buffer(0);
        result = jfgui_start((int32_t)target_info.width, (int32_t)target_info.height, dpi, 0);
        started = result == 0;
    } else {
        if (last_dpi != dpi) jfgui_event(9, 0, 0, 0, 0, 0, (uint32_t)dpi);
        if (new_surface || last_width != (int)target_info.width || last_height != (int)target_info.height)
            jfgui_event(8, 0, 0, 0, (int32_t)target_info.width, (int32_t)target_info.height, 0);
    }
    if (started) {
        last_width = (int)target_info.width;
        last_height = (int)target_info.height;
        last_dpi = dpi;
        jfgui_tick(time_ms());
    }
    target_bitmap = NULL;
    target_env = NULL;
    return result != 0 ? -1 : presented;
}

JNIEXPORT jint JNICALL Java_org_jfgui_gallery_GalleryActivity_nativeFrameDelay(JNIEnv *env, jclass cls) {
    (void)env; (void)cls;
    return started ? (jint)jfgui_frame_delay() : 250;
}
JNIEXPORT jbyteArray JNICALL Java_org_jfgui_gallery_GalleryActivity_nativeEditorState(JNIEnv *env, jclass cls) {
    (void)cls;
    uint32_t length = jfgui_editor_snapshot();
    if (length > 262160) return NULL;
    jbyteArray out = (*env)->NewByteArray(env, (jsize)length);
    if (out) (*env)->SetByteArrayRegion(env, out, 0, (jsize)length, (const jbyte *)jfgui_editor_data());
    return out;
}
JNIEXPORT void JNICALL Java_org_jfgui_gallery_GalleryActivity_nativeEditorReplace(
        JNIEnv *env, jclass cls, jint id, jbyteArray text, jint anchor, jint caret) {
    (void)cls;
    if (!started || !text) return;
    jsize length = (*env)->GetArrayLength(env, text);
    if (length < 0 || length > 262144) return;
    uintptr_t buffer = jfgui_buffer((uint32_t)length);
    if (!buffer) return;
    (*env)->GetByteArrayRegion(env, text, 0, length, (jbyte *)buffer);
    if ((*env)->ExceptionCheck(env)) return;
    if (jfgui_editor_replace((uint32_t)id, (uint32_t)length, (uint32_t)anchor, (uint32_t)caret))
        jfgui_input(time_ms());
}

JNIEXPORT void JNICALL Java_org_jfgui_gallery_GalleryActivity_nativeEvent(
        JNIEnv *env, jclass cls, jint type, jint button, jint mods, jint x, jint y, jint data) {
    (void)env; (void)cls;
    if (!started) return;
    jfgui_event((uint32_t)type, (uint32_t)button, (uint32_t)mods, 0, x, y, (uint32_t)data);
    jfgui_input(time_ms());
}
JNIEXPORT void JNICALL Java_org_jfgui_gallery_GalleryActivity_nativeText(
        JNIEnv *env, jclass cls, jbyteArray text) {
    (void)cls;
    if (!started || !text) return;
    jsize length = (*env)->GetArrayLength(env, text);
    if (length <= 0 || length > 262144) return;
    uintptr_t buffer = jfgui_buffer((uint32_t)length);
    if (!buffer) return;
    (*env)->GetByteArrayRegion(env, text, 0, length, (jbyte *)buffer);
    if ((*env)->ExceptionCheck(env)) return;
    jfgui_text((uint32_t)length);
    jfgui_input(time_ms());
}
JNIEXPORT jboolean JNICALL Java_org_jfgui_gallery_GalleryActivity_nativeIme(
        JNIEnv *env, jclass cls) {
    (void)env; (void)cls;
    return ime_visible ? JNI_TRUE : JNI_FALSE;
}
