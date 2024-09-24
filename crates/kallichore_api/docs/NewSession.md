# NewSession

## Properties
Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**session_id** | **String** | A unique identifier for the session | 
**display_name** | **String** | A human-readable name for the session | 
**language** | **String** | The interpreter language | 
**username** | **String** | The username of the user who owns the session | 
**argv** | **Vec<String>** | The program and command-line parameters for the session | 
**working_directory** | **String** | The working directory in which to start the session. | 
**env** | **std::collections::HashMap<String, String>** | Environment variables to set for the session | 
**interrupt_mode** | [***models::InterruptMode**](interrupt_mode.md) |  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


