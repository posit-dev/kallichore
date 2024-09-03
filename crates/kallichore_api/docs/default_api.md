# default_api

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
**channels-websocket**](default_api.md#channels-websocket) | **GET** /sessions/{session_id}/channels | Upgrade to a WebSocket for channel communication
**kill-session**](default_api.md#kill-session) | **GET** /sessions/{session_id}/kill | Force quit session
**list-sessions**](default_api.md#list-sessions) | **GET** /sessions | List active sessions
**new-session**](default_api.md#new-session) | **PUT** /sessions | Create a new session
**start-session**](default_api.md#start-session) | **GET** /sessions/{session_id}/start | Start a session


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
> models::NewSession200Response new-session(session)
Create a new session

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session** | [**Session**](Session.md)|  | 

### Return type

[**models::NewSession200Response**](new_session_200_response.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
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

