#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$ROOT"

MODEL_DIR="models/lfm"
MODEL_FILE="$MODEL_DIR/lfmf16.gguf"
MODEL_URL="https://huggingface.co/LiquidAI/LFM2-350M-ENJP-MT-GGUF/resolve/main/LFM2-350M-ENJP-MT-F16.gguf?download=true"
MODEL_SHA256="0ed20b653006d4e97dd2b4a22872971e45fa71ad6ccb3cd21705747ee08d3b30"

JM_GZ="JMdict_e_examp.gz"
JM_URL="http://ftp.edrdg.org/pub/Nihongo/JMdict_e_examp.gz"
JM_FILE="jm.xml"

download() {
    url=$1
    output=$2

    if [ -f "$output" ]; then
        echo "Already exists: $output"
        return
    fi

    tmp="${output}.part"
    rm -f "$tmp"

    echo "Downloading $output ..."
    if ! curl -fL --retry 3 --retry-delay 2 "$url" -o "$tmp"; then
        rm -f "$tmp"
        echo "Download failed: $output" >&2
        exit 1
    fi

    mv "$tmp" "$output"
}

verify_sha256() {
    file=$1
    expected=$2

    if command -v sha256sum >/dev/null 2>&1; then
        printf '%s  %s\n' "$expected" "$file" | sha256sum -c -
    elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "$file" | awk '{print $1}')
        if [ "$actual" != "$expected" ]; then
            echo "SHA-256 mismatch for $file" >&2
            exit 1
        fi
    else
        echo "Warning: no SHA-256 utility found; skipping model verification." >&2
    fi
}

mkdir -p "$MODEL_DIR"

# 1. Sudachi dictionary
if [ ! -f system.dic ]; then
    echo "Fetching Sudachi system dictionary..."
    sh ./fetch_dictionary.sh
else
    echo "Already exists: system.dic"
fi

# 2. Translation model
if [ ! -f "$MODEL_FILE" ]; then
    download "$MODEL_URL" "$MODEL_FILE"
    verify_sha256 "$MODEL_FILE" "$MODEL_SHA256"
else
    echo "Already exists: $MODEL_FILE"
    verify_sha256 "$MODEL_FILE" "$MODEL_SHA256"
fi

# 3. JMdict English dictionary with example sentences
if [ ! -f "$JM_FILE" ]; then
    download "$JM_URL" "$JM_GZ"

    echo "Extracting JMdict..."
    if ! gzip -dc "$JM_GZ" > "$JM_FILE.part"; then
        rm -f "$JM_FILE.part"
        echo "Could not decompress JMdict archive." >&2
        exit 1
    fi

    mv "$JM_FILE.part" "$JM_FILE"
    rm -f "$JM_GZ"
else
    echo "Already exists: $JM_FILE"
fi


# ── llama.cpp CPU and CUDA builds ─────────────────────────────

LLAMA_BUILD="b11856"
LLAMA_DIR="tools/llama.cpp"
LLAMA_RELEASE_URL="https://github.com/ggml-org/llama.cpp/releases/download/${LLAMA_BUILD}"

download_and_extract() {
    archive="$1"
    dest="$2"
    url="${LLAMA_RELEASE_URL}/${archive}"
    part="${archive}.part"

    echo "Downloading ${archive}..."

    if ! curl -fL --retry 3 --retry-delay 2 "$url" -o "$part"; then
        rm -f "$part"
        echo "Download failed: ${archive}" >&2
        return 1
    fi

    if ! unzip -t "$part" >/dev/null; then
        rm -f "$part"
        echo "Invalid ZIP archive: ${archive}" >&2
        return 1
    fi

    mkdir -p "$dest"

    if ! unzip -o "$part" -d "$dest"; then
        rm -f "$part"
        echo "Extraction failed: ${archive}" >&2
        return 1
    fi

    rm -f "$part"
    echo "Installed ${archive}"
}

# CPU executable and its DLLs.
if [ ! -f "$LLAMA_DIR/cpu/llama-server.exe" ]; then
    download_and_extract \
        "llama-${LLAMA_BUILD}-bin-win-cpu-x64.zip" \
        "$LLAMA_DIR/cpu" || exit 1
else
    echo "Already exists: $LLAMA_DIR/cpu/llama-server.exe"
fi

# CUDA executable and its bundled files.
if [ ! -f "$LLAMA_DIR/cuda/llama-server.exe" ]; then
    download_and_extract \
        "llama-${LLAMA_BUILD}-bin-win-cuda-12.4-x64.zip" \
        "$LLAMA_DIR/cuda" || exit 1
else
    echo "Already exists: $LLAMA_DIR/cuda/llama-server.exe"
fi

# CUDA runtime DLLs are distributed separately.
# Always extract this archive so missing DLLs can be restored.
download_and_extract \
    "cudart-llama-bin-win-cuda-12.4-x64.zip" \
    "$LLAMA_DIR/cuda" || exit 1

# Verify the important files are present.
for file in \
    "$LLAMA_DIR/cpu/llama-server.exe" \
    "$LLAMA_DIR/cuda/llama-server.exe" \
    "$LLAMA_DIR/cuda/ggml-cuda.dll" \
    "$LLAMA_DIR/cuda/cublas64_12.dll" \
    "$LLAMA_DIR/cuda/cublasLt64_12.dll" \
    "$LLAMA_DIR/cuda/cudart64_12.dll"
do
    if [ ! -f "$file" ]; then
        echo "Required llama.cpp file is missing: $file" >&2
        exit 1
    fi
done

echo "llama.cpp CPU and CUDA builds are ready."

echo
echo "All assets are ready."
