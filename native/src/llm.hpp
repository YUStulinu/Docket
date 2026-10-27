// The local language model on the phone: llama.cpp, CPU only, answering in JSON that a grammar
// (made from the core's JSON schema) constrains token by token.
#pragma once

#include <string>
#include <vector>

struct llama_model;
struct llama_context;

namespace docket {

class Llm {
public:
    // Loads a GGUF model. `context`: tokens of prompt plus answer; `threads`: 0 for a guess.
    Llm(const std::string& path, int context = 4096, int threads = 0);
    ~Llm();
    Llm(const Llm&) = delete;
    Llm& operator=(const Llm&) = delete;

    // The answer to `user` under `system`, matching `json_schema` (empty: free text).
    std::string complete(const std::string& system, const std::string& user, const std::string& json_schema, int max_tokens = 1024);

    const std::string& name() const { return name_; }

private:
    llama_model* model_ = nullptr;
    llama_context* ctx_ = nullptr;
    std::string name_;
    // The tokens in the context's memory: a new prompt that starts the same way (the system
    // prompt is the same for every document) is only computed from where it differs.
    std::vector<int> cached_;
};

}  // namespace docket
