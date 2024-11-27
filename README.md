# Chappie
A simple command line tool that takes a text or file, performs a fuzzy search, and then asks the questions you care about in the large language model.
store the results in vectorbase, generate embeddings 

# Get start

# Commands
* `llm`: Choose a large language model API currently only supports `groq` and `gemini`,env is CHAP_LLM_NAME
* `model`: Large language model types such as llama3-8b-8192,env is CHAP_LLM_MODEL
* `api_key`: remote api API key,env is CHAP_LLM_API_KEY
* `ui`: UI type full or lite,env is CHAP_UI
* `vb`: Whether to use vector database to save results,env is CHAP_VB

# operate
* `up`: Previous row of text
* `down`: Next row of text
* `ctrl+up`: Previous page of text
* `ctrl+down`: Next page of text
* `tab`: Switch focus between text and chat
* `shift+up`: Select Previous row
* `shift+down`: Select next row
* `ctrl+x`: Exit and display the results on the terminal