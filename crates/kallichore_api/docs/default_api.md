# default_api

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
**adopt-session**](default_api.md#adopt-session) | **PUT** /sessions/adopt | Adopt an existing session
**channels-websocket**](default_api.md#channels-websocket) | **GET** /sessions/{session_id}/channels | Upgrade to a WebSocket for channel communication
**delete-session**](default_api.md#delete-session) | **DELETE** /sessions/{session_id} | Delete session
**get-session**](default_api.md#get-session) | **GET** /sessions/{session_id} | Get session details
**interrupt-session**](default_api.md#interrupt-session) | **POST** /sessions/{session_id}/interrupt | Interrupt session
**kill-session**](default_api.md#kill-session) | **POST** /sessions/{session_id}/kill | Force quit session
**list-sessions**](default_api.md#list-sessions) | **GET** /sessions | List active sessions
**new-session**](default_api.md#new-session) | **PUT** /sessions | Create a new session
**restart-session**](default_api.md#restart-session) | **POST** /sessions/{session_id}/restart | Restart a session
**server-status**](default_api.md#server-status) | **GET** /status | Get server status and information
**shutdown-server**](default_api.md#shutdown-server) | **POST** /shutdown | Shut down all sessions and the server itself
**start-session**](default_api.md#start-session) | **POST** /sessions/{session_id}/start | Start a session


# **adopt-session**
> models::NewSession200Response adopt-session(adopted_session)
Adopt an existing session

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **adopted_session** | [**AdoptedSession**](AdoptedSession.md)|  | 

### Return type

[**models::NewSession200Response**](new_session_200_response.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **channels-websocket**
> channels-websocket(session_id)
Upgrade to a WebSocket for channel communication

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **delete-session**
> serde_json::Value delete-session(session_id)
Delete session

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 

### Return type

[**serde_json::Value**](AnyType.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **get-session**
> models::ActiveSession get-session(session_id)
Get session details

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 

### Return type

[**models::ActiveSession**](activeSession.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **interrupt-session**
> serde_json::Value interrupt-session(session_id)
Interrupt session

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 

### Return type

[**serde_json::Value**](AnyType.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **kill-session**
> serde_json::Value kill-session(session_id)
Force quit session

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 

### Return type

[**serde_json::Value**](AnyType.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **list-sessions**
> models::SessionList list-sessions()
List active sessions

### Required Parameters
This endpoint does not need any parameter.

### Return type

[**models::SessionList**](sessionList.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **new-session**
> models::NewSession200Response new-session(new_session)
Create a new session

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **new_session** | [**NewSession**](NewSession.md)|  | 

### Return type

[**models::NewSession200Response**](new_session_200_response.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **restart-session**
> serde_json::Value restart-session(session_id)
Restart a session

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 

### Return type

[**serde_json::Value**](AnyType.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **server-status**
> models::ServerStatus server-status()
Get server status and information

### Required Parameters
This endpoint does not need any parameter.

### Return type

[**models::ServerStatus**](serverStatus.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **shutdown-server**
> serde_json::Value shutdown-server()
Shut down all sessions and the server itself

### Required Parameters
This endpoint does not need any parameter.

### Return type

[**serde_json::Value**](AnyType.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **start-session**
> serde_json::Value start-session(session_id)
Start a session

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 

### Return type

[**serde_json::Value**](AnyType.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

