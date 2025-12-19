FROM golang:1.23-alpine AS builder

WORKDIR /app

# Install git for fetch dependencies
RUN apk add --no-cache git

COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN go build -o gateway .

FROM alpine:latest

WORKDIR /app
COPY --from=builder /app/gateway .

# Create data directories mostly for local testing/permissions, 
# though volume mount will override
RUN mkdir -p /app/data/raw

EXPOSE 8080

CMD ["./gateway"]
