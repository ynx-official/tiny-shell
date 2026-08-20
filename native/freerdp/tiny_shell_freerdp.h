#ifndef TINY_SHELL_FREERDP_H
#define TINY_SHELL_FREERDP_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct tiny_shell_rdp_client tiny_shell_rdp_client;

typedef void (*tiny_shell_rdp_state_callback)(void* user_data, uint32_t state,
                                               uint32_t error_code,
                                               const char* message);
typedef void (*tiny_shell_rdp_frame_callback)(void* user_data, uint32_t width,
                                               uint32_t height, uint32_t stride,
                                               const uint8_t* pixels, size_t length);
typedef int (*tiny_shell_rdp_should_stop_callback)(void* user_data);
typedef void (*tiny_shell_rdp_poll_callback)(void* user_data);
typedef uint32_t (*tiny_shell_rdp_certificate_callback)(
    void* user_data, const char* host, uint16_t port, const char* common_name,
    const char* subject, const char* issuer, const char* fingerprint, uint32_t flags);
typedef uint32_t (*tiny_shell_rdp_changed_certificate_callback)(
    void* user_data, const char* host, uint16_t port, const char* common_name,
    const char* subject, const char* issuer, const char* fingerprint,
    const char* old_subject, const char* old_issuer, const char* old_fingerprint,
    uint32_t flags);
typedef void (*tiny_shell_rdp_clipboard_callback)(void* user_data,
                                                  const uint8_t* text,
                                                  size_t length);

typedef struct tiny_shell_rdp_callbacks {
    void* user_data;
    tiny_shell_rdp_state_callback on_state;
    tiny_shell_rdp_frame_callback on_frame;
    tiny_shell_rdp_should_stop_callback should_stop;
    tiny_shell_rdp_poll_callback on_poll;
    tiny_shell_rdp_certificate_callback on_certificate;
    tiny_shell_rdp_changed_certificate_callback on_changed_certificate;
    tiny_shell_rdp_clipboard_callback on_clipboard;
} tiny_shell_rdp_callbacks;

typedef struct tiny_shell_rdp_config {
    const char* host;
    uint16_t port;
    const char* username;
    const char* password;
    const char* domain;
    uint32_t width;
    uint32_t height;
} tiny_shell_rdp_config;

enum tiny_shell_rdp_state {
    TINY_SHELL_RDP_STATE_CONNECTING = 1,
    TINY_SHELL_RDP_STATE_CONNECTED = 2,
    TINY_SHELL_RDP_STATE_DISCONNECTED = 3,
    TINY_SHELL_RDP_STATE_FAILED = 4,
};

tiny_shell_rdp_client* tiny_shell_rdp_client_new(
    const tiny_shell_rdp_config* config,
    const tiny_shell_rdp_callbacks* callbacks);

int tiny_shell_rdp_client_run(tiny_shell_rdp_client* client);
int tiny_shell_rdp_client_resize(tiny_shell_rdp_client* client, uint32_t width,
                                 uint32_t height);
int tiny_shell_rdp_client_keyboard(tiny_shell_rdp_client* client, int down,
                                   int extended, uint32_t scancode);
int tiny_shell_rdp_client_text(tiny_shell_rdp_client* client, const uint16_t* text,
                               size_t length);
int tiny_shell_rdp_client_clipboard(tiny_shell_rdp_client* client,
                                    const uint16_t* text, size_t length);
int tiny_shell_rdp_client_clipboard_files(tiny_shell_rdp_client* client,
                                          const uint8_t* paths, size_t length);
int tiny_shell_rdp_client_mouse(tiny_shell_rdp_client* client, uint16_t flags,
                                uint16_t x, uint16_t y);
void tiny_shell_rdp_client_stop(tiny_shell_rdp_client* client);
int tiny_shell_rdp_client_should_retry(tiny_shell_rdp_client* client,
                                       int result);
void tiny_shell_rdp_client_free(tiny_shell_rdp_client* client);

#ifdef __cplusplus
}
#endif

#endif
