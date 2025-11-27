#!/bin/bash
# Install models for whisper.cpp
mkdir -p .models
wget https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin?download=true -O .models/ggml-small-fp16.bin