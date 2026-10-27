#include "llm.hpp"

#include <algorithm>
#include <filesystem>
#include <stdexcept>
#include <thread>
#include <vector>

#include "json-schema-to-grammar.h"
#include "llama.h"

namespace docket {

Llm::Llm(const std::string& path, int context, int threads) {
    llama_backend_init();
    llama_model_params mp = llama_model_default_params();
    // The default load mode maps the file: the weights stay in the page cache, not the app's heap.
    model_ = llama_model_load_from_file(path.c_str(), mp);
    if (!model_) throw std::runtime_error("cannot load the model " + path);
    llama_context_params cp = llama_context_default_params();
    cp.n_ctx = static_cast<uint32_t>(context);
    cp.n_batch = 512;
    int n = threads > 0 ? threads : std::max(2, static_cast<int>(std::thread::hardware_concurrency()) - 2);
    cp.n_threads = n;
    cp.n_threads_batch = n;
    ctx_ = llama_init_from_model(model_, cp);
    if (!ctx_) {
        llama_model_free(model_);
        throw std::runtime_error("cannot create a context for the model");
    }
    name_ = std::filesystem::path(path).filename().string();
}

Llm::~Llm() {
    if (ctx_) llama_free(ctx_);
    if (model_) llama_model_free(model_);
}

std::string Llm::complete(const std::string& system, const std::string& user, const std::string& json_schema, int max_tokens) {
    const llama_vocab* vocab = llama_model_get_vocab(model_);

    // The model's own chat template (ChatML for Qwen).
    llama_chat_message msgs[] = {{"system", system.c_str()}, {"user", user.c_str()}};
    const char* tmpl = llama_model_chat_template(model_, nullptr);
    std::vector<char> buf(system.size() + user.size() + 1024);
    int len = llama_chat_apply_template(tmpl, msgs, 2, true, buf.data(), static_cast<int32_t>(buf.size()));
    if (len > static_cast<int>(buf.size())) {
        buf.resize(len + 1);
        len = llama_chat_apply_template(tmpl, msgs, 2, true, buf.data(), static_cast<int32_t>(buf.size()));
    }
    if (len < 0) throw std::runtime_error("the model has no usable chat template");
    std::string prompt(buf.data(), len);

    std::vector<llama_token> tokens(prompt.size() + 16);
    int n = llama_tokenize(vocab, prompt.c_str(), static_cast<int32_t>(prompt.size()), tokens.data(), static_cast<int32_t>(tokens.size()), true, true);
    if (n < 0) throw std::runtime_error("tokenizing the prompt failed");
    tokens.resize(n);
    if (n + max_tokens > static_cast<int>(llama_n_ctx(ctx_))) {
        throw std::runtime_error("the document is too long for the model's context");
    }

    llama_sampler* smpl = llama_sampler_chain_init(llama_sampler_chain_default_params());
    if (!json_schema.empty()) {
        std::string grammar = json_schema_to_grammar(common_json::parse(json_schema));
        llama_sampler* g = llama_sampler_init_grammar(vocab, grammar.c_str(), "root");
        if (!g) {
            llama_sampler_free(smpl);
            throw std::runtime_error("the JSON schema did not make a grammar");
        }
        llama_sampler_chain_add(smpl, g);
    }
    llama_sampler_chain_add(smpl, llama_sampler_init_greedy());  // deterministic, like temperature 0

    // Reuse what the memory already holds of this prompt; keep at least the last token to decode,
    // for its logits.
    size_t common = 0;
    while (common < cached_.size() && common < tokens.size() && cached_[common] == tokens[common]) ++common;
    if (common == tokens.size()) --common;
    llama_memory_t mem = llama_get_memory(ctx_);
    if (!llama_memory_seq_rm(mem, 0, static_cast<llama_pos>(common), -1)) {
        llama_memory_clear(mem, true);
        common = 0;
    }
    cached_.assign(tokens.begin(), tokens.begin() + static_cast<long>(common));
    std::string out;
    // The rest of the prompt in batches, then one token at a time.
    for (int i = static_cast<int>(common); i < n; i += 512) {
        int k = std::min(512, n - i);
        if (llama_decode(ctx_, llama_batch_get_one(tokens.data() + i, k)) != 0) {
            llama_sampler_free(smpl);
            cached_.clear();
            llama_memory_clear(mem, true);
            throw std::runtime_error("decoding the prompt failed");
        }
        cached_.insert(cached_.end(), tokens.begin() + i, tokens.begin() + i + k);
    }
    for (int i = 0; i < max_tokens; ++i) {
        llama_token t = llama_sampler_sample(smpl, ctx_, -1);
        if (llama_vocab_is_eog(vocab, t)) break;
        char piece[256];
        int m = llama_token_to_piece(vocab, t, piece, sizeof piece, 0, true);
        if (m > 0) out.append(piece, m);
        if (llama_decode(ctx_, llama_batch_get_one(&t, 1)) != 0) break;
        cached_.push_back(t);
    }
    llama_sampler_free(smpl);
    return out;
}

}  // namespace docket
