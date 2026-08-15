#include "tiny_shell_freerdp.h"

#include <freerdp/client.h>
#include <freerdp/client/cliprdr.h>
#include <freerdp/client/cmdline.h>
#include <freerdp/client/disp.h>
#include <freerdp/channels/disp.h>
#include <freerdp/codec/color.h>
#include <freerdp/error.h>
#include <freerdp/freerdp.h>
#include <freerdp/gdi/gdi.h>
#include <freerdp/input.h>
#include <freerdp/settings.h>
#include <winpr/crt.h>
#include <winpr/interlocked.h>
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
    volatile LONG certificate_rejected;
    DispClientContext* disp;
    CliprdrClientContext* cliprdr;
    uint16_t* clipboard_text;
    size_t clipboard_length;
    uint32_t pending_width;
    uint32_t pending_height;
    BOOL disp_ready;
};

static UINT ts_cliprdr_monitor_ready(CliprdrClientContext* cliprdr,
                                     const CLIPRDR_MONITOR_READY* ready);
static UINT ts_cliprdr_server_capabilities(CliprdrClientContext* cliprdr,
                                           const CLIPRDR_CAPABILITIES* capabilities);
static UINT ts_cliprdr_server_format_list(CliprdrClientContext* cliprdr,
                                          const CLIPRDR_FORMAT_LIST* format_list);
static UINT ts_cliprdr_server_format_data_request(
    CliprdrClientContext* cliprdr, const CLIPRDR_FORMAT_DATA_REQUEST* request);
static UINT ts_cliprdr_server_format_data_response(
    CliprdrClientContext* cliprdr, const CLIPRDR_FORMAT_DATA_RESPONSE* response);
static UINT ts_disp_caps(DispClientContext* disp, UINT32 max_monitors,
                         UINT32 area_factor_a, UINT32 area_factor_b);

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

static void ts_channel_connected(void* value, const ChannelConnectedEventArgs* event)
{
    tiny_shell_rdp_context* context = (tiny_shell_rdp_context*)value;
    tiny_shell_rdp_client* client = context ? context->client : NULL;
    if (!client || !event || !event->name)
        return;
    if (strcmp(event->name, DISP_DVC_CHANNEL_NAME) == 0)
    {
        client->disp = (DispClientContext*)event->pInterface;
        if (client->disp)
        {
            client->disp->custom = client;
            client->disp->DisplayControlCaps = ts_disp_caps;
        }
    }
    else if (strcmp(event->name, CLIPRDR_SVC_CHANNEL_NAME) == 0)
    {
        client->cliprdr = (CliprdrClientContext*)event->pInterface;
        if (client->cliprdr)
        {
            client->cliprdr->custom = client;
            client->cliprdr->MonitorReady = ts_cliprdr_monitor_ready;
            client->cliprdr->ServerCapabilities = ts_cliprdr_server_capabilities;
            client->cliprdr->ServerFormatList = ts_cliprdr_server_format_list;
            client->cliprdr->ServerFormatDataRequest = ts_cliprdr_server_format_data_request;
            client->cliprdr->ServerFormatDataResponse = ts_cliprdr_server_format_data_response;
        }
    }
}

static void ts_channel_disconnected(void* value, const ChannelDisconnectedEventArgs* event)
{
    tiny_shell_rdp_context* context = (tiny_shell_rdp_context*)value;
    tiny_shell_rdp_client* client = context ? context->client : NULL;
    if (!client || !event || !event->name)
        return;
    if (strcmp(event->name, DISP_DVC_CHANNEL_NAME) == 0)
    {
        client->disp = NULL;
        client->disp_ready = FALSE;
    }
    else if (strcmp(event->name, CLIPRDR_SVC_CHANNEL_NAME) == 0)
        client->cliprdr = NULL;
}

static BOOL ts_load_channels(freerdp* instance)
{
    if (!instance || !instance->context)
        return FALSE;
    return freerdp_client_load_addins(instance->context->channels,
                                      instance->context->settings);
}

static UINT ts_disp_caps(DispClientContext* disp, UINT32 max_monitors,
                         UINT32 area_factor_a, UINT32 area_factor_b)
{
    tiny_shell_rdp_client* client = disp ? (tiny_shell_rdp_client*)disp->custom : NULL;
    (void)max_monitors;
    (void)area_factor_a;
    (void)area_factor_b;
    if (!client)
        return CHANNEL_RC_BAD_CHANNEL_HANDLE;
    client->disp_ready = TRUE;
    return CHANNEL_RC_OK;
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
    if (!freerdp_settings_set_bool(settings, FreeRDP_DynamicResolutionUpdate, TRUE) ||
        !freerdp_settings_set_bool(settings, FreeRDP_RedirectClipboard, TRUE))
        return FALSE;
    if (PubSub_SubscribeChannelConnected(instance->context->pubSub,
                                         ts_channel_connected) < 0 ||
        PubSub_SubscribeChannelDisconnected(instance->context->pubSub,
                                            ts_channel_disconnected) < 0)
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

static DWORD ts_verify_certificate(freerdp* instance, const char* host, UINT16 port,
                                   const char* common_name, const char* subject,
                                   const char* issuer, const char* fingerprint, DWORD flags)
{
    tiny_shell_rdp_context* context = NULL;
    if (!instance || !instance->context)
        return 0;
    context = (tiny_shell_rdp_context*)instance->context;
    if (!context->client || !context->client->callbacks.on_certificate)
        return 0;
    DWORD result = context->client->callbacks.on_certificate(
        context->client->callbacks.user_data, host ? host : "", port,
        common_name ? common_name : "", subject ? subject : "", issuer ? issuer : "",
        fingerprint ? fingerprint : "", flags);
    if (result == 0)
        InterlockedExchange(&context->client->certificate_rejected, 1);
    return result;
}

static DWORD ts_verify_changed_certificate(
    freerdp* instance, const char* host, UINT16 port, const char* common_name,
    const char* subject, const char* issuer, const char* fingerprint,
    const char* old_subject, const char* old_issuer, const char* old_fingerprint,
    DWORD flags)
{
    tiny_shell_rdp_context* context = NULL;
    if (!instance || !instance->context)
        return 0;
    context = (tiny_shell_rdp_context*)instance->context;
    if (!context->client || !context->client->callbacks.on_changed_certificate)
        return 0;
    DWORD result = context->client->callbacks.on_changed_certificate(
        context->client->callbacks.user_data, host ? host : "", port,
        common_name ? common_name : "", subject ? subject : "", issuer ? issuer : "",
        fingerprint ? fingerprint : "", old_subject ? old_subject : "",
        old_issuer ? old_issuer : "", old_fingerprint ? old_fingerprint : "", flags);
    if (result == 0)
        InterlockedExchange(&context->client->certificate_rejected, 1);
    return result;
}

static UINT ts_cliprdr_send_capabilities(CliprdrClientContext* cliprdr)
{
    CLIPRDR_CAPABILITIES capabilities = { 0 };
    CLIPRDR_GENERAL_CAPABILITY_SET general = { 0 };
    if (!cliprdr || !cliprdr->ClientCapabilities)
        return CHANNEL_RC_BAD_CHANNEL_HANDLE;
    capabilities.cCapabilitiesSets = 1;
    capabilities.capabilitySets = (CLIPRDR_CAPABILITY_SET*)&general;
    general.capabilitySetType = CB_CAPSTYPE_GENERAL;
    general.capabilitySetLength = CB_CAPSTYPE_GENERAL_LEN;
    general.version = CB_CAPS_VERSION_2;
    general.generalFlags = CB_USE_LONG_FORMAT_NAMES;
    return cliprdr->ClientCapabilities(cliprdr, &capabilities);
}

static UINT ts_cliprdr_send_format_list(CliprdrClientContext* cliprdr)
{
    CLIPRDR_FORMAT format = { 0 };
    CLIPRDR_FORMAT_LIST list = { 0 };
    if (!cliprdr || !cliprdr->ClientFormatList)
        return CHANNEL_RC_BAD_CHANNEL_HANDLE;
    format.formatId = CF_UNICODETEXT;
    list.numFormats = 1;
    list.formats = &format;
    return cliprdr->ClientFormatList(cliprdr, &list);
}

static UINT ts_cliprdr_monitor_ready(CliprdrClientContext* cliprdr,
                                     const CLIPRDR_MONITOR_READY* ready)
{
    (void)ready;
    UINT result = ts_cliprdr_send_capabilities(cliprdr);
    if (result != CHANNEL_RC_OK)
        return result;
    return ts_cliprdr_send_format_list(cliprdr);
}

static UINT ts_cliprdr_server_capabilities(CliprdrClientContext* cliprdr,
                                           const CLIPRDR_CAPABILITIES* capabilities)
{
    (void)cliprdr;
    (void)capabilities;
    return CHANNEL_RC_OK;
}

static UINT ts_cliprdr_server_format_list(CliprdrClientContext* cliprdr,
                                          const CLIPRDR_FORMAT_LIST* format_list)
{
    CLIPRDR_FORMAT_LIST_RESPONSE response = { 0 };
    CLIPRDR_FORMAT_DATA_REQUEST request = { 0 };
    BOOL supports_unicode = FALSE;
    if (!cliprdr || !format_list || !cliprdr->ClientFormatListResponse)
        return CHANNEL_RC_BAD_CHANNEL_HANDLE;
    for (UINT32 index = 0; index < format_list->numFormats; index++)
    {
        if (format_list->formats[index].formatId == CF_UNICODETEXT)
        {
            supports_unicode = TRUE;
            break;
        }
    }
    response.common.msgFlags = CB_RESPONSE_OK;
    UINT result = cliprdr->ClientFormatListResponse(cliprdr, &response);
    if (result != CHANNEL_RC_OK || !supports_unicode ||
        !cliprdr->ClientFormatDataRequest)
        return result;
    request.requestedFormatId = CF_UNICODETEXT;
    return cliprdr->ClientFormatDataRequest(cliprdr, &request);
}

static UINT ts_cliprdr_server_format_data_request(
    CliprdrClientContext* cliprdr, const CLIPRDR_FORMAT_DATA_REQUEST* request)
{
    CLIPRDR_FORMAT_DATA_RESPONSE response = { 0 };
    tiny_shell_rdp_client* client = cliprdr ? (tiny_shell_rdp_client*)cliprdr->custom : NULL;
    if (!cliprdr || !request || !cliprdr->ClientFormatDataResponse)
        return CHANNEL_RC_BAD_CHANNEL_HANDLE;
    if (!client || request->requestedFormatId != CF_UNICODETEXT || !client->clipboard_text)
    {
        response.common.msgFlags = CB_RESPONSE_FAIL;
    }
    else
    {
        response.common.msgFlags = CB_RESPONSE_OK;
        response.common.dataLen = (UINT32)((client->clipboard_length + 1) * sizeof(uint16_t));
        response.requestedFormatData = (const BYTE*)client->clipboard_text;
    }
    return cliprdr->ClientFormatDataResponse(cliprdr, &response);
}

static UINT ts_cliprdr_server_format_data_response(
    CliprdrClientContext* cliprdr, const CLIPRDR_FORMAT_DATA_RESPONSE* response)
{
    tiny_shell_rdp_client* client = cliprdr ? (tiny_shell_rdp_client*)cliprdr->custom : NULL;
    if (!client || !response || (response->common.msgFlags & CB_RESPONSE_FAIL) ||
        !response->requestedFormatData || !client->callbacks.on_clipboard)
        return CHANNEL_RC_OK;
    client->callbacks.on_clipboard(client->callbacks.user_data,
                                   response->requestedFormatData,
                                   response->common.dataLen);
    return CHANNEL_RC_OK;
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
    context->instance->LoadChannels = ts_load_channels;
    context->instance->VerifyCertificateEx = ts_verify_certificate;
    context->instance->VerifyChangedCertificateEx = ts_verify_changed_certificate;
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
        (void)freerdp_client_stop(context);
        return (int)error_code;
    }
    connected = TRUE;
    ts_emit_state(client, TINY_SHELL_RDP_STATE_CONNECTED, 0, "connected");

    while (!InterlockedCompareExchange(&client->stop_requested, 0, 0) &&
           (!client->callbacks.should_stop ||
            !client->callbacks.should_stop(client->callbacks.user_data)) &&
           !freerdp_shall_disconnect_context(context))
    {
        if (client->callbacks.on_poll)
            client->callbacks.on_poll(client->callbacks.user_data);
        if (client->disp_ready && client->pending_width >= DISPLAY_CONTROL_MIN_MONITOR_WIDTH &&
            client->pending_height >= DISPLAY_CONTROL_MIN_MONITOR_HEIGHT)
            (void)tiny_shell_rdp_client_resize(client, client->pending_width,
                                               client->pending_height);
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

int tiny_shell_rdp_client_resize(tiny_shell_rdp_client* client, uint32_t width,
                                 uint32_t height)
{
    DISPLAY_CONTROL_MONITOR_LAYOUT layout = { 0 };
    rdpSettings* settings = NULL;
    if (!client || !client->context || !client->context->settings ||
        width < DISPLAY_CONTROL_MIN_MONITOR_WIDTH ||
        width > DISPLAY_CONTROL_MAX_MONITOR_WIDTH ||
        height < DISPLAY_CONTROL_MIN_MONITOR_HEIGHT ||
        height > DISPLAY_CONTROL_MAX_MONITOR_HEIGHT)
        return 0;
    client->pending_width = width;
    client->pending_height = height;
    if (!client->disp || !client->disp_ready || !client->disp->SendMonitorLayout)
        return 1;
    settings = client->context->settings;
    layout.Flags = DISPLAY_CONTROL_MONITOR_PRIMARY;
    layout.Width = width;
    layout.Height = height;
    layout.Orientation = freerdp_settings_get_uint16(settings, FreeRDP_DesktopOrientation);
    layout.DesktopScaleFactor =
        freerdp_settings_get_uint32(settings, FreeRDP_DesktopScaleFactor);
    layout.DeviceScaleFactor =
        freerdp_settings_get_uint32(settings, FreeRDP_DeviceScaleFactor);
    if (client->disp->SendMonitorLayout(client->disp, 1, &layout) != CHANNEL_RC_OK)
    {
        client->pending_width = 0;
        client->pending_height = 0;
        return 0;
    }
    client->pending_width = 0;
    client->pending_height = 0;
    return 1;
}

int tiny_shell_rdp_client_keyboard(tiny_shell_rdp_client* client, int down,
                                   int extended, uint32_t scancode)
{
    if (!client || !client->context || !client->context->input || scancode > 0xFF)
        return 0;
    scancode = MAKE_RDP_SCANCODE(scancode, extended ? TRUE : FALSE);
    return freerdp_input_send_keyboard_event_ex(client->context->input,
                                                down ? TRUE : FALSE, FALSE, scancode)
               ? 1
               : 0;
}

int tiny_shell_rdp_client_text(tiny_shell_rdp_client* client, const uint16_t* text,
                               size_t length)
{
    if (!client || !client->context || !client->context->input || !text || length == 0)
        return 0;
    for (size_t index = 0; index < length; index++)
    {
        if (!freerdp_input_send_unicode_keyboard_event(client->context->input, 0, text[index]) ||
            !freerdp_input_send_unicode_keyboard_event(client->context->input, KBD_FLAGS_RELEASE,
                                                       text[index]))
            return 0;
    }
    return 1;
}

int tiny_shell_rdp_client_clipboard(tiny_shell_rdp_client* client,
                                    const uint16_t* text, size_t length)
{
    uint16_t* copy = NULL;
    if (!client || !text || length == 0 || length > (8U * 1024U * 1024U) / sizeof(uint16_t) ||
        length > (UINT32_MAX / sizeof(uint16_t)) - 1 ||
        length > (SIZE_MAX / sizeof(uint16_t)) - 1)
        return 0;
    copy = (uint16_t*)calloc(length + 1, sizeof(uint16_t));
    if (!copy)
        return 0;
    memcpy(copy, text, length * sizeof(uint16_t));
    if (client->clipboard_text)
    {
        SecureZeroMemory(client->clipboard_text,
                         (client->clipboard_length + 1) * sizeof(uint16_t));
        free(client->clipboard_text);
    }
    client->clipboard_text = copy;
    client->clipboard_length = length;
    return client->cliprdr && ts_cliprdr_send_format_list(client->cliprdr) == CHANNEL_RC_OK;
}

int tiny_shell_rdp_client_mouse(tiny_shell_rdp_client* client, uint16_t flags,
                                uint16_t x, uint16_t y)
{
    if (!client || !client->context || !client->context->input)
        return 0;
    return freerdp_input_send_mouse_event(client->context->input, flags, x, y) ? 1 : 0;
}

void tiny_shell_rdp_client_stop(tiny_shell_rdp_client* client)
{
    if (!client)
        return;
    InterlockedExchange(&client->stop_requested, 1);
    if (client->context)
        (void)freerdp_abort_connect_context(client->context);
}

int tiny_shell_rdp_client_should_retry(tiny_shell_rdp_client* client, int result)
{
    if (!client || InterlockedCompareExchange(&client->stop_requested, 0, 0) ||
        InterlockedCompareExchange(&client->certificate_rejected, 0, 0))
        return 0;
    switch ((uint32_t)result)
    {
        case FREERDP_ERROR_AUTHENTICATION_FAILED:
        case FREERDP_ERROR_INSUFFICIENT_PRIVILEGES:
        case FREERDP_ERROR_CONNECT_PASSWORD_EXPIRED:
        case FREERDP_ERROR_CONNECT_PASSWORD_CERTAINLY_EXPIRED:
        case FREERDP_ERROR_CONNECT_ACCOUNT_DISABLED:
        case FREERDP_ERROR_CONNECT_PASSWORD_MUST_CHANGE:
        case FREERDP_ERROR_CONNECT_LOGON_FAILURE:
        case FREERDP_ERROR_CONNECT_WRONG_PASSWORD:
        case FREERDP_ERROR_CONNECT_ACCESS_DENIED:
        case FREERDP_ERROR_CONNECT_ACCOUNT_RESTRICTION:
        case FREERDP_ERROR_CONNECT_ACCOUNT_LOCKED_OUT:
        case FREERDP_ERROR_CONNECT_ACCOUNT_EXPIRED:
        case FREERDP_ERROR_CONNECT_LOGON_TYPE_NOT_GRANTED:
        case FREERDP_ERROR_CONNECT_NO_OR_MISSING_CREDENTIALS:
        case FREERDP_ERROR_CONNECT_CANCELLED:
            return 0;
        default:
            return 1;
    }
}

void tiny_shell_rdp_client_free(tiny_shell_rdp_client* client)
{
    if (!client)
        return;
    if (client->context)
        freerdp_client_context_free(client->context);
    free((void*)client->config.host);
    free((void*)client->config.username);
    if (client->config.password)
    {
        SecureZeroMemory((void*)client->config.password, strlen(client->config.password));
        free((void*)client->config.password);
    }
    free((void*)client->config.domain);
    if (client->clipboard_text)
    {
        SecureZeroMemory(client->clipboard_text,
                         (client->clipboard_length + 1) * sizeof(uint16_t));
        free(client->clipboard_text);
    }
    free(client);
}
