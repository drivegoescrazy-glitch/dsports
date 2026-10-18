#include <jni.h>
#include <string>

namespace dsports {

// This is deliberately a bootstrap, not a DS emulator or a SoulSilver port.
// Native game logic and DS hardware services must be implemented separately.
constexpr int kRuntimeApiVersion = 1;

std::string status() {
    return "Native core loaded (API " + std::to_string(kRuntimeApiVersion) +
           "). SoulSilver gameplay is not implemented yet.";
}

}  // namespace dsports

extern "C" JNIEXPORT jstring JNICALL
Java_com_dsports_library_NativePortBridge_nativeStatus(JNIEnv* env, jobject /*thiz*/) {
    const std::string message = dsports::status();
    return env->NewStringUTF(message.c_str());
}
