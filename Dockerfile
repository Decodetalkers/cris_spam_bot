FROM rust:1.80.1

WORKDIR /app

COPY . .

RUN cargo install --path .

ENTRYPOINT [ "cris_spam_bot" ]
