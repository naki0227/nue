package main

import (
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"go.uber.org/zap"
)

func main() {
	// Initialize Zap logger for JSON output
	logger, _ := zap.NewProduction()
	defer logger.Sync()

	// Create a gin router with custom logging middleware to use Zap
	r := gin.New()
	r.Use(func(c *gin.Context) {
		start := time.Now()
		c.Next()
		logger.Info("request",
			zap.String("method", c.Request.Method),
			zap.String("path", c.Request.URL.Path),
			zap.Int("status", c.Writer.Status()),
			zap.Duration("latency", time.Since(start)),
		)
	})

	// Add CORS middleware
	r.Use(func(c *gin.Context) {
		c.Writer.Header().Set("Access-Control-Allow-Origin", "*")
		c.Writer.Header().Set("Access-Control-Allow-Credentials", "true")
		c.Writer.Header().Set("Access-Control-Allow-Headers", "Content-Type, Content-Length, Accept-Encoding, X-CSRF-Token, Authorization, accept, origin, Cache-Control, X-Requested-With")
		c.Writer.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS, GET, PUT")

		if c.Request.Method == "OPTIONS" {
			c.AbortWithStatus(204)
			return
		}

		c.Next()
	})

	uploadPath := "/app/data/raw"
	if _, err := os.Stat(uploadPath); os.IsNotExist(err) {
		uploadPath = "../data/raw" // Fallback
		os.MkdirAll(uploadPath, 0755)
	}

	r.POST("/upload", func(c *gin.Context) {
		file, err := c.FormFile("file")
		if err != nil {
			logger.Error("upload_failed", zap.Error(err))
			c.JSON(http.StatusBadRequest, gin.H{"error": "No file is received"})
			return
		}

		// Get metadata if provided
		metadata := c.PostForm("metadata")

		ext := filepath.Ext(file.Filename)
		newFilename := uuid.New().String() + ext
		dst := filepath.Join(uploadPath, newFilename)

		if err := c.SaveUploadedFile(file, dst); err != nil {
			logger.Error("save_failed", zap.String("filename", newFilename), zap.Error(err))
			c.JSON(http.StatusInternalServerError, gin.H{"error": fmt.Sprintf("Unable to save file: %s", err.Error())})
			return
		}

		// Save metadata if provided
		if metadata != "" {
			metadataPath := filepath.Join(uploadPath, newFilename+"_metadata.json")
			if err := os.WriteFile(metadataPath, []byte(metadata), 0644); err != nil {
				logger.Error("metadata_save_failed", zap.String("filename", newFilename), zap.Error(err))
				// Don't fail the whole upload if metadata save fails
			} else {
				logger.Info("metadata_saved", zap.String("filename", newFilename))
			}
		}

		logger.Info("file_received",
			zap.String("original_name", file.Filename),
			zap.String("stored_name", newFilename),
			zap.Bool("has_metadata", metadata != ""),
		)

		// 202 Accepted because processing happens asynchronously by other services
		c.JSON(http.StatusAccepted, gin.H{
			"message":  "File uploaded successfully",
			"filename": newFilename,
			"status":   "processing_started",
		})
	})

	logger.Info("service_starting", zap.String("port", "8080"))
	r.Run(":8080")
}
