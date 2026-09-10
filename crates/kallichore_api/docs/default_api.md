# default_api

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
**client-heartbeat**](default_api.md#client-heartbeat) | **POST** /client_heartbeat | Notify the server that a client is connected
**get-server-configuration**](default_api.md#get-server-configuration) | **GET** /server_configuration | Get the server configuration
**list-sessions**](default_api.md#list-sessions) | **GET** /sessions | List active sessions
**new-session**](default_api.md#new-session) | **PUT** /sessions | Create a new session
**register-mcp-frontend**](default_api.md#register-mcp-frontend) | **POST** /mcp/frontends | Register a Positron frontend with the MCP server
**server-status**](default_api.md#server-status) | **GET** /status | Get server status and information
**set-server-configuration**](default_api.md#set-server-configuration) | **POST** /server_configuration | Change the server configuration
**shutdown-server**](default_api.md#shutdown-server) | **POST** /shutdown | Shut down all sessions and the server itself
**adopt-session**](default_api.md#adopt-session) | **PUT** /sessions/{session_id}/adopt | Adopt an existing session
**channels-upgrade**](default_api.md#channels-upgrade) | **GET** /sessions/{session_id}/channels | Upgrade to a WebSocket or domain socket for channel communication
**connection-info**](default_api.md#connection-info) | **GET** /sessions/{session_id}/connection_info | Get Jupyter connection information for the session
**delete-session**](default_api.md#delete-session) | **DELETE** /sessions/{session_id} | Delete session
**deregister-mcp-frontend**](default_api.md#deregister-mcp-frontend) | **DELETE** /mcp/frontends/{frontend_id} | Deregister a Positron frontend
**execute-code**](default_api.md#execute-code) | **POST** /sessions/{session_id}/execute | Execute code and return results
**get-session**](default_api.md#get-session) | **GET** /sessions/{session_id} | Get session details
**interrupt-session**](default_api.md#interrupt-session) | **POST** /sessions/{session_id}/interrupt | Interrupt session
**kill-session**](default_api.md#kill-session) | **POST** /sessions/{session_id}/kill | Force quit session
**mcp-frontend-channel**](default_api.md#mcp-frontend-channel) | **GET** /mcp/frontends/{frontend_id}/channel | Upgrade to a WebSocket carrying the MCP frontend channel
**restart-session**](default_api.md#restart-session) | **POST** /sessions/{session_id}/restart | Restart a session
**start-session**](default_api.md#start-session) | **POST** /sessions/{session_id}/start | Start a session


# **client-heartbeat**
> serde_json::Value client-heartbeat(client_heartbeat)
Notify the server that a client is connected

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **client_heartbeat** | [**ClientHeartbeat**](ClientHeartbeat.md)|  | 

### Return type

[**serde_json::Value**](AnyType.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **get-server-configuration**
> models::ServerConfiguration get-server-configuration()
Get the server configuration

### Required Parameters
This endpoint does not need any parameter.

### Return type

[**models::ServerConfiguration**](serverConfiguration.md)

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

# **register-mcp-frontend**
> models::McpFrontend register-mcp-frontend(mcp_frontend_registration)
Register a Positron frontend with the MCP server

Registers (or re-registers) a frontend and starts the MCP listener if it isn't already running. Re-registering with a known frontend ID returns the same bearer token, so agents launched from terminals that outlived the frontend keep working.

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **mcp_frontend_registration** | [**McpFrontendRegistration**](McpFrontendRegistration.md)|  | 

### Return type

[**models::McpFrontend**](mcpFrontend.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
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

# **set-server-configuration**
> serde_json::Value set-server-configuration(server_configuration)
Change the server configuration

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **server_configuration** | [**ServerConfiguration**](ServerConfiguration.md)|  | 

### Return type

[**serde_json::Value**](AnyType.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
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

# **adopt-session**
> serde_json::Value adopt-session(session_id, connection_info)
Adopt an existing session

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 
  **connection_info** | [**ConnectionInfo**](ConnectionInfo.md)|  | 

### Return type

[**serde_json::Value**](AnyType.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **channels-upgrade**
> String channels-upgrade(session_id)
Upgrade to a WebSocket or domain socket for channel communication

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 

### Return type

[**String**](string.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **connection-info**
> models::ConnectionInfo connection-info(session_id)
Get Jupyter connection information for the session

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 

### Return type

[**models::ConnectionInfo**](connectionInfo.md)

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

# **deregister-mcp-frontend**
> deregister-mcp-frontend(frontend_id)
Deregister a Positron frontend

Removes the frontend and invalidates its token. When the last frontend is removed the MCP listener stops and its port is released.

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **frontend_id** | **String**|  | 

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **execute-code**
> models::ExecuteReply execute-code(session_id, execute_request)
Execute code and return results

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 
  **execute_request** | [**ExecuteRequest**](ExecuteRequest.md)|  | 

### Return type

[**models::ExecuteReply**](executeReply.md)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: application/json
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

# **mcp-frontend-channel**
> mcp-frontend-channel(frontend_id)
Upgrade to a WebSocket carrying the MCP frontend channel

Opens the bidirectional channel over which the frontend pushes its command catalog and foreground session, and over which the supervisor brokers agent command requests.

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **frontend_id** | **String**|  | 

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **restart-session**
> serde_json::Value restart-session(session_id, optional)
Restart a session

### Required Parameters

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
  **session_id** | **String**|  | 
 **optional** | **map[string]interface{}** | optional parameters | nil if no parameters

### Optional Parameters
Optional parameters are passed through a map[string]interface{}.

Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **session_id** | **String**|  | 
 **restart_session** | [**RestartSession**](RestartSession.md)|  | 

### Return type

[**serde_json::Value**](AnyType.md)

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

