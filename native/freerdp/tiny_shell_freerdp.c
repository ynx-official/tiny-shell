#include "tiny_shell_freerdp.h"

#include <freerdp/client.h>
#include <freerdp/codec/color.h>
#include <freerdp/freerdp.h>
#include <freerdp/gdi/gdi.h>
#include <freerdp/settings.h>
#include <winpr/crt.h>
#include <winpr/synch.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct tiny_shell_rdp_context {
    rdpContext context;
    tiny_shell_rdp_client* client;
} tiny_shell_rdp_context;

struct tiny_shell_rdp_client {
    rdpContext* context;
    tiny_shell_rdp_config config;
    tiny_shell_rdp_callbacks callbacks;
    volatile LONG stop_requested;
};

static char* ts_strdup(const char* value)
{
    size_t length = 0;
    char* copy = NULL;

    if (!value)
        return NULL;
    length = strlen(value) + 1;
    copy = (char*)calloc(length, sizeof(char));
    if (copy)
        memcpy(copy, value, length);
    return copy;
}

static void ts_emit_state(tiny_shell_rdp_client* client, uint32_t state,
                          uint32_t error_code, const char* message)
{
    if (client && client->callbacks.on_state)
        client->callbacks.on_state(client->callbacks.user_data, state,
                                   error_code, message ? message : "");
}

static BOOL ts_set_string(rdpSettings* settings, const char* key, const char* value)
{
    if (!value || value[0] == '\0')
        return TRUE;
    return freerdp_settings_set_value_for_name(settings, key, value);
}

static BOOL ts_pre_connect(freerdp* instance)
{
    tiny_shell_rdp_context* context = NULL;
    tiny_shell_rdp_client* client = NULL;
    rdpSettings* settings = NULL;
    char port[16] = { 0 };
    char width[16] = { 0 };
    char height[16] = { 0 };

    if (!instance || !instance->context)
        return FALSE;
    context = (tiny_shell_rdp_context*)instance->context;
    client = context->client;
    settings = instance->context->settings;
    if (!client || !settings)
        return FALSE;

    (void)snprintf(port, sizeof(port), "%u", (unsigned)client->config.port);
    (void)snprintf(width, sizeof(width), "%u", (unsigned)client->config.width);
    (void)snprintf(height, sizeof(height), "%u", (unsigned)client->config.height);

    if (!ts_set_string(settings, "FreeRDP_ServerHostname", client->config.host))
        return FALSE;
    if (!ts_set_string(settings, "FreeRDP_ServerPort", port))
        return FALSE;
    if (!ts_set_string(settings, "FreeRDP_Username", client->config.username))
        return FALSE;
    if (!ts_set_string(settings, "FreeRDP_Password", client->config.password))
        return FALSE;
    if (!ts_set_string(settings, "FreeRDP_Domain", client->config.domain))
        return FALSE;
    if (client->config.width > 0 &&
        !ts_set_string(settings, "FreeRDP_DesktopWidth", width))
        return FALSE;
    if (client->config.height > 0 &&
        !ts_set_string(settings, "FreeRDP_DesktopHeight", height))
        return FALSE;

    /* Prefer the graphics pipeline and AVC when the linked FreeRDP build has
     * these capabilities. Unknown settings are rejected by FreeRDP, so these
     * options are intentionally best-effort for distro-specific builds. */
    (void)freerdp_settings_set_value_for_name(settings,
                                               "FreeRDP_SupportGraphicsPipeline", "TRUE");
    (void)freerdp_settings_set_value_for_name(settings, "FreeRDP_GfxH264", "TRUE");
    return TRUE;
}

static BOOL ts_begin_paint(rdpContext* context)
{
    rdpGdi* gdi = NULL;
    if (!context || !context->gdi || !context->gdi->primary ||
        !context->gdi->primary->hdc || !context->gdi->primary->hdc->hwnd ||
        !context->gdi->primary->hdc->hwnd->invalid)
        return FALSE;
    gdi = context->gdi;
    gdi->primary->hdc->hwnd->invalid->null = TRUE;
    return TRUE;
}

static BOOL ts_end_paint(rdpContext* context)
{
    tiny_shell_rdp_context* ts_context = NULL;
    rdpGdi* gdi = NULL;
    size_t length = 0;

    if (!context || !context->gdi || !context->instance)
        return FALSE;
    ts_context = (tiny_shell_rdp_context*)context;
    gdi = context->gdi;
    if (!gdi->primary_buffer || gdi->width <= 0 || gdi->height <= 0 ||
        gdi->stride == 0 || !ts_context->client ||
        !ts_context->client->callbacks.on_frame)
        return TRUE;

    length = (size_t)gdi->stride * (size_t)gdi->height;
    ts_context->client->callbacks.on_frame(
        ts_context->client->callbacks.user_data,
        (uint32_t)gdi->width,
        (uint32_t)gdi->height,
        gdi->stride,
        gdi->primary_buffer,
        length);
    return TRUE;
}

static BOOL ts_desktop_resize(rdpContext* context)
{
    rdpSettings* settings = NULL;
    if (!context || !context->gdi || !context->settings)
        return FALSE;
    settings = context->settings;
    return gdi_resize(context->gdi,
                      freerdp_settings_get_uint32(settings, FreeRDP_DesktopWidth),
                      freerdp_settings_get_uint32(settings, FreeRDP_DesktopHeight));
}

static BOOL ts_post_connect(freerdp* instance)
{
    if (!instance || !instance->context)
        return FALSE;
    if (!gdi_init(instance, PIXEL_FORMAT_BGRA32))
        return FALSE;
    if (!instance->context->update) {
        gdi_free(instance);
        return FALSE;
    }
    instance->context->update->BeginPaint = ts_begin_paint;
    instance->context->update->EndPaint = ts_end_paint;
    instance->context->update->DesktopResize = ts_desktop_resize;
    return TRUE;
}

static void ts_post_disconnect(freerdp* instance)
{
    if (instance && instance->context && instance->context->gdi)
        gdi_free(instance);
}

static BOOL ts_client_new(freerdp* instance, rdpContext* context)
{
    (void)instance;
    (void)context;
    return TRUE;
}

static void ts_client_free(freerdp* instance, rdpContext* context)
{
    (void)instance;
    (void)context;
}

static int ts_client_start(rdpContext* context)
{
    (void)context;
    return 0;
}

static int ts_client_stop(rdpContext* context)
{
    (void)context;
    return 0;
}

static void ts_entry_points(RDP_CLIENT_ENTRY_POINTS* entry_points)
{
    ZeroMemory(entry_points, sizeof(*entry_points));
    entry_points->Version = RDP_CLIENT_INTERFACE_VERSION;
    entry_points->Size = sizeof(RDP_CLIENT_ENTRY_POINTS_V1);
    entry_points->ContextSize = sizeof(tiny_shell_rdp_context);
    entry_points->ClientNew = ts_client_new;
    entry_points->ClientFree = ts_client_free;
    entry_points->ClientStart = ts_client_start;
    entry_points->ClientStop = ts_client_stop;
}

tiny_shell_rdp_client* tiny_shell_rdp_client_new(
    const tiny_shell_rdp_config* config,
    const tiny_shell_rdp_callbacks* callbacks)
{
    tiny_shell_rdp_client* client = NULL;
    RDP_CLIENT_ENTRY_POINTS entry_points = { 0 };
    rdpContext* context = NULL;

    if (!config || !config->host || config->host[0] == '\0')
        return NULL;
    client = (tiny_shell_rdp_client*)calloc(1, sizeof(*client));
    if (!client)
        return NULL;
    client->config.host = ts_strdup(config->host);
    client->config.username = ts_strdup(config->username);
    client->config.password = ts_strdup(config->password);
    client->config.domain = ts_strdup(config->domain);
    client->config.port = config->port ? config->port : 3389;
    client->config.width = config->width;
    client->config.height = config->height;
    if (!client->config.host ||
        (config->username && !client->config.username) ||
        (config->password && !client->config.password) ||
        (config->domain && !client->config.domain))
        goto fail;
    if (callbacks)
        client->callbacks = *callbacks;

    ts_entry_points(&entry_points);
    context = freerdp_client_context_new(&entry_points);
    if (!context)
        goto fail;
    ((tiny_shell_rdp_context*)context)->client = client;
    client->context = context;
    context->instance->PreConnect = ts_pre_connect;
    context->instance->PostConnect = ts_post_connect;
    context->instance->PostDisconnect = ts_post_disconnect;
    return client;

fail:
    tiny_shell_rdp_client_free(client);
    return NULL;
}

int tiny_shell_rdp_client_run(tiny_shell_rdp_client* client)
{
    freerdp* instance = NULL;
    rdpContext* context = NULL;
    HANDLE handles[MAXIMUM_WAIT_OBJECTS] = { 0 };
    DWORD count = 0;
    BOOL connected = FALSE;
    uint32_t error_code = 0;

    if (!client || !client->context || !client->context->instance)
        return -1;
    instance = client->context->instance;
    context = client->context;
    InterlockedExchange(&client->stop_requested, 0);
    ts_emit_state(client, TINY_SHELL_RDP_STATE_CONNECTING, 0, "connecting");

    if (freerdp_client_start(context) != 0 || !freerdp_connect(instance))
    {
        error_code = freerdp_get_last_error(context);
        ts_emit_state(client, TINY_SHELL_RDP_STATE_FAILED, error_code,
                      freerdp_get_last_error_string(error_code));
        return (int)error_code;
    }
    connected = TRUE;
    ts_emit_state(client, TINY_SHELL_RDP_STATE_CONNECTED, 0, "connected");

    while (!InterlockedCompareExchange(&client->stop_requested, 0, 0) &&
           (!client->callbacks.should_stop ||
            !client->callbacks.should_stop(client->callbacks.user_data)) &&
           !freerdp_shall_disconnect_context(context))
    {
        count = freerdp_get_event_handles(context, handles, ARRAYSIZE(handles));
        if (count == 0)
            break;
        if (WaitForMultipleObjects(count, handles, FALSE, 100) == WAIT_FAILED)
            break;
        if (!freerdp_check_event_handles(context))
            break;
    }

    freerdp_disconnect(instance);
    if (connected)
        ts_emit_state(client, TINY_SHELL_RDP_STATE_DISCONNECTED, 0, "disconnected");
    (void)freerdp_client_stop(context);
    return 0;
}

void tiny_shell_rdp_client_stop(tiny_shell_rdp_client* client)
{
    if (!client)
        return;
    InterlockedExchange(&client->stop_requested, 1);
    if (client->context)
        (void)freerdp_abort_connect_context(client->context);
}

void tiny_shell_rdp_client_free(tiny_shell_rdp_client* client)
{
    if (!client)
        return;
    if (client->context)
        freerdp_client_context_free(client->context);
    free((void*)client->config.host);
    free((void*)client->config.username);
    free((void*)client->config.password);
    free((void*)client->config.domain);
    free(client);
}
