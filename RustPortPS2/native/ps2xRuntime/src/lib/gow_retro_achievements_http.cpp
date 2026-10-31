// RetroAchievements HTTPS transport with WinHTTP (see gow_retro_achievements_http.h).
#include "gow_retro_achievements_http.h"

#include "rc_api_request.h"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <winhttp.h>
#include <bcrypt.h>
#include <vector>

namespace
{
    std::wstring toWide(const std::string &text)
    {
        if (text.empty())
            return {};
        const int size = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text.data(), static_cast<int>(text.size()), nullptr, 0);
        if (size <= 0)
            return {};
        std::wstring out(static_cast<size_t>(size), L'\0');
        MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text.data(), static_cast<int>(text.size()), out.data(), size);
        return out;
    }
}

GowRaHttpResponse gowRaHttpRequest(const std::string &urlUtf8, const std::string &post, const std::string &contentType)
{
    GowRaHttpResponse out;
    out.status = RC_API_SERVER_RESPONSE_CLIENT_ERROR;
    const std::wstring url = toWide(urlUtf8);
    if (url.empty())
    {
        out.body = "Invalid RetroAchievements URL";
        return out;
    }

    URL_COMPONENTS parts{};
    parts.dwStructSize = sizeof(parts);
    parts.dwSchemeLength = static_cast<DWORD>(-1);
    parts.dwHostNameLength = static_cast<DWORD>(-1);
    parts.dwUrlPathLength = static_cast<DWORD>(-1);
    parts.dwExtraInfoLength = static_cast<DWORD>(-1);
    if (!WinHttpCrackUrl(url.c_str(), static_cast<DWORD>(url.size()), 0, &parts))
    {
        out.body = "Could not parse the RetroAchievements URL";
        return out;
    }

    const std::wstring host(parts.lpszHostName, parts.dwHostNameLength);
    std::wstring target(parts.lpszUrlPath, parts.dwUrlPathLength);
    if (parts.lpszExtraInfo && parts.dwExtraInfoLength)
        target.append(parts.lpszExtraInfo, parts.dwExtraInfoLength);
    if (target.empty())
        target = L"/";

    HINTERNET session = WinHttpOpen(L"PC-GodOfWar/1.0", WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                                   WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0);
    if (!session)
    {
        out.body = "Could not open the HTTPS connection";
        return out;
    }
    WinHttpSetTimeouts(session, 7000, 7000, 15000, 15000);
    HINTERNET connection = WinHttpConnect(session, host.c_str(), parts.nPort, 0);
    if (!connection)
    {
        WinHttpCloseHandle(session);
        out.body = "Could not connect to RetroAchievements";
        return out;
    }

    const bool sendPost = !post.empty();
    const DWORD flags = parts.nScheme == INTERNET_SCHEME_HTTPS ? WINHTTP_FLAG_SECURE : 0;
    HINTERNET request = WinHttpOpenRequest(connection, sendPost ? L"POST" : L"GET", target.c_str(), nullptr,
                                          WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES, flags);
    if (!request)
    {
        WinHttpCloseHandle(connection);
        WinHttpCloseHandle(session);
        out.body = "Could not create the request to RetroAchievements";
        return out;
    }

    std::wstring header;
    if (sendPost)
        header = L"Content-Type: " + toWide(contentType.empty() ? "application/x-www-form-urlencoded" : contentType);
    const BOOL sent = WinHttpSendRequest(request, header.empty() ? WINHTTP_NO_ADDITIONAL_HEADERS : header.c_str(),
                                            header.empty() ? 0 : static_cast<DWORD>(header.size()),
                                            sendPost ? const_cast<char *>(post.data()) : WINHTTP_NO_REQUEST_DATA,
                                            sendPost ? static_cast<DWORD>(post.size()) : 0,
                                            sendPost ? static_cast<DWORD>(post.size()) : 0, 0);
    if (sent && WinHttpReceiveResponse(request, nullptr))
    {
        DWORD code = 0;
        DWORD codeSize = sizeof(code);
        WinHttpQueryHeaders(request, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_HEADER_NAME_BY_INDEX,
                            &code, &codeSize, WINHTTP_NO_HEADER_INDEX);
        out.status = static_cast<int>(code);
        for (;;)
        {
            DWORD available = 0;
            if (!WinHttpQueryDataAvailable(request, &available) || available == 0)
                break;
            const size_t before = out.body.size();
            out.body.resize(before + available);
            DWORD read = 0;
            if (!WinHttpReadData(request, out.body.data() + before, available, &read))
            {
                out.body.resize(before);
                out.status = RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR;
                break;
            }
            out.body.resize(before + read);
        }
    }
    else
    {
        out.body = "Network failure talking to RetroAchievements";
        out.status = RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR;
    }
    WinHttpCloseHandle(request);
    WinHttpCloseHandle(connection);
    WinHttpCloseHandle(session);
    return out;
}

bool gowRaMd5WithPrefix(const std::string &prefix, const std::wstring &file, std::string &hex)
{
    HANDLE handle = CreateFileW(file.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING,
                             FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
    if (handle == INVALID_HANDLE_VALUE)
        return false;

    BCRYPT_ALG_HANDLE alg = nullptr;
    BCRYPT_HASH_HANDLE hash = nullptr;
    bool ok = BCryptOpenAlgorithmProvider(&alg, BCRYPT_MD5_ALGORITHM, nullptr, 0) >= 0 &&
              BCryptCreateHash(alg, &hash, nullptr, 0, nullptr, 0, 0) >= 0 &&
              BCryptHashData(hash, reinterpret_cast<PUCHAR>(const_cast<char *>(prefix.data())),
                             static_cast<ULONG>(prefix.size()), 0) >= 0;
    std::vector<unsigned char> buffer(1u << 20);
    while (ok)
    {
        DWORD read = 0;
        if (!ReadFile(handle, buffer.data(), static_cast<DWORD>(buffer.size()), &read, nullptr))
        {
            ok = false;
            break;
        }
        if (read == 0)
            break;
        ok = BCryptHashData(hash, buffer.data(), read, 0) >= 0;
    }
    unsigned char digest[16]{};
    ok = ok && BCryptFinishHash(hash, digest, sizeof(digest), 0) >= 0;
    if (hash)
        BCryptDestroyHash(hash);
    if (alg)
        BCryptCloseAlgorithmProvider(alg, 0);
    CloseHandle(handle);
    if (!ok)
        return false;

    static const char digits[] = "0123456789abcdef";
    hex.clear();
    for (unsigned char b : digest)
    {
        hex.push_back(digits[b >> 4]);
        hex.push_back(digits[b & 15]);
    }
    return true;
}
#else
bool gowRaMd5WithPrefix(const std::string &, const std::wstring &, std::string &)
{
    return false;
}

GowRaHttpResponse gowRaHttpRequest(const std::string &, const std::string &, const std::string &)
{
    GowRaHttpResponse out;
    out.status = RC_API_SERVER_RESPONSE_CLIENT_ERROR;
    out.body = "RetroAchievements requires the Windows HTTPS implementation in this port";
    return out;
}
#endif
