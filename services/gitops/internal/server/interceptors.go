package server

import (
	"context"
	"log/slog"
	"runtime/debug"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// Interceptors log every call at debug (method, code, duration; never a
// body) and turn a panic into INTERNAL instead of a dead process.
func Interceptors(log *slog.Logger) (grpc.UnaryServerInterceptor, grpc.StreamServerInterceptor) {
	unary := func(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (resp any, err error) {
		start := time.Now()
		defer func() {
			if r := recover(); r != nil {
				log.Error("panic in handler", "method", info.FullMethod, "panic", r, "stack", string(debug.Stack()))
				err = status.Error(codes.Internal, "internal error")
			}
			log.Debug("rpc", "method", info.FullMethod, "code", status.Code(err).String(), "ms", time.Since(start).Milliseconds())
		}()
		return handler(ctx, req)
	}
	stream := func(srv any, ss grpc.ServerStream, info *grpc.StreamServerInfo, handler grpc.StreamHandler) (err error) {
		start := time.Now()
		defer func() {
			if r := recover(); r != nil {
				log.Error("panic in stream handler", "method", info.FullMethod, "panic", r, "stack", string(debug.Stack()))
				err = status.Error(codes.Internal, "internal error")
			}
			log.Debug("rpc stream", "method", info.FullMethod, "code", status.Code(err).String(), "ms", time.Since(start).Milliseconds())
		}()
		return handler(srv, ss)
	}
	return unary, stream
}
