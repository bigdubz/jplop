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

# ── llama.cpp Linux CPU and CUDA builds ────────────────────────

LLAMA_BUILD="b11856"
LLAMA_DIR="tools/llama.cpp"
LLAMA_RELEASE_URL="https://github.com/ggml-org/llama.cpp/releases/download/${LLAMA_BUILD}"


download_and_extract() {
    archive="$1"
    dest="$2"
    url="${LLAMA_RELEASE_URL}/${archive}"
    part="${archive}.part"
    temp_dir=$(mktemp -d) || return 1

    echo "Downloading ${archive}..."

    if ! curl -fL --retry 3 --retry-delay 2 "$url" -o "$part"; then
        rm -f "$part"
        rm -rf "$temp_dir"
        echo "Download failed: ${archive}" >&2
        return 1
    fi

    if ! tar -tzf "$part" >/dev/null; then
        rm -f "$part"
        rm -rf "$temp_dir"
        echo "Invalid tar.gz archive: ${archive}" >&2
        return 1
    fi

    if ! tar -xzf "$part" -C "$temp_dir"; then
        rm -f "$part"
        rm -rf "$temp_dir"
        echo "Extraction failed: ${archive}" >&2
        return 1
    fi

    mkdir -p "$dest"

    if [ "$archive" = "cudart-llama-${LLAMA_BUILD}-bin-ubuntu-cuda-12.8-x64.tar.gz" ]; then
        # Flatten only the CUDA runtime archive into cuda/.
        find "$temp_dir" -type f -exec cp -f -t "$dest" -- {} + || {
            rm -f "$part"
            rm -rf "$temp_dir"
            echo "CUDA runtime installation failed: ${archive}" >&2
            return 1
        }
    else
        # Preserve the original directory structure for llama.cpp builds.
        cp -a "$temp_dir/." "$dest/" || {
            rm -f "$part"
            rm -rf "$temp_dir"
            echo "Extraction installation failed: ${archive}" >&2
            return 1
        }
    fi

    rm -f "$part"
    rm -rf "$temp_dir"
    echo "Installed ${archive}"
}

# CPU executable and shared libraries.
if [ ! -f "$LLAMA_DIR/cpu/llama-server" ]; then
    download_and_extract \
        "llama-${LLAMA_BUILD}-bin-ubuntu-x64.tar.gz" \
        "$LLAMA_DIR/cpu" || exit 1
else
    echo "Already exists: $LLAMA_DIR/cpu/llama-server"
fi

# CUDA executable and shared libraries.
if [ ! -f "$LLAMA_DIR/cuda/llama-server" ]; then
    download_and_extract \
        "llama-${LLAMA_BUILD}-bin-ubuntu-cuda-12.8-x64.tar.gz" \
        "$LLAMA_DIR/cuda" || exit 1
else
    echo "Already exists: $LLAMA_DIR/cuda/llama-server"
fi

# CUDA runtime libraries: flatten these beside llama-server too.
download_and_extract \
    "cudart-llama-${LLAMA_BUILD}-bin-ubuntu-cuda-12.8-x64.tar.gz" \
    "$LLAMA_DIR/cuda" || exit 1

mv tools/llama.cpp/cpu/llama-b11856/* tools/llama.cpp/cpu/
rmdir tools/llama.cpp/cpu/llama-b11856

mv tools/llama.cpp/cuda/llama-b11856/* tools/llama.cpp/cuda/
rmdir tools/llama.cpp/cuda/llama-b11856

# Verify the important files are present.
for file in \
    "$LLAMA_DIR/cpu/llama-server" \
    "$LLAMA_DIR/cuda/llama-server" \
    "$LLAMA_DIR/cuda/libggml-cuda.so" \
    "$LLAMA_DIR/cuda/libcudart.so.12" \
    "$LLAMA_DIR/cuda/libcublas.so.12"
do
    if [ ! -f "$file" ]; then
        echo "Required llama.cpp file is missing: $file" >&2
        exit 1
    fi
done

chmod +x "$LLAMA_DIR/cpu/llama-server"
chmod +x "$LLAMA_DIR/cuda/llama-server"

echo "llama.cpp Linux CPU and CUDA builds are ready."
echo
echo "All assets are ready."
