# ExecuteReply

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**status** | [***models::ExecuteReplyStatus**](executeReply_status.md) |  | 
**execution_count** | **i32** | The kernel's execution counter | 
**output** | [**Vec<models::ExecuteOutput>**](executeOutput.md) | All output messages produced during execution, in order | 
**data** | **std::collections::HashMap<String, String>** | The execution result as a MIME-keyed dictionary (from execute_result), if the execution produced a result | [optional] [default to None]
**error_name** | **String** | The error name, if the execution failed | [optional] [default to None]
**error_message** | **String** | The error message, if the execution failed | [optional] [default to None]
**error_traceback** | **Vec<String>** | The error traceback, if the execution failed | [optional] [default to None]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


