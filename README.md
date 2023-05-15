# rust-gpt-tokenizer

A rusty implementation of [this Javascript tokenizer](https://github.com/latitudegames/GPT-3-Encoder), which is itself based off [OpenAI's own implementation](https://github.com/openai/gpt-2).

I tried to mirror the original implementation 1:1 where possible. That means this tokenizer is surely not as fast as it could be. Also, I had quite a blast (to put it this way) modifying the lengthy files that GPT uses for tokenization (located in /lib) since various compatibility issues arose between javascript's regex engine and pcre2.
