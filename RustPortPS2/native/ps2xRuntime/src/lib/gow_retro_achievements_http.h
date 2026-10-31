// RetroAchievements HTTPS transport. Kept separate from the integration because
// windows.h/winhttp.h collide with raylib.h (CloseWindow, ShowCursor, DrawText).
#pragma once

#include <string>

struct GowRaHttpResponse
{
    int status = 0; // HTTP status code, or an RC_API_SERVER_RESPONSE_* when the network fails
    std::string body;
};

// Blocking: always call outside the game thread. GET when `post` is empty.
GowRaHttpResponse gowRaHttpRequest(const std::string &urlUtf8, const std::string &post, const std::string &contentType);

// MD5 (lowercase hex) of `prefix` followed by the contents of `file`. This is RetroAchievements' PS2 hash:
// boot executable name + ELF bytes. Returns false if the file could not be read.
bool gowRaMd5WithPrefix(const std::string &prefix, const std::wstring &file, std::string &hex);
