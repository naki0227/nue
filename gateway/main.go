package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"go.uber.org/zap"
)

type jobStatus struct {
	JobID            string `json:"job_id"`
	Status           string `json:"status"`
	OriginalFilename string `json:"original_filename"`
	StoredFilename   string `json:"stored_filename"`
	Message          string `json:"message"`
	Progress         int    `json:"progress"`
	UpdatedAt        string `json:"updated_at"`
}

func writeJobStatus(jobsPath string, status jobStatus) error {
	status.UpdatedAt = time.Now().UTC().Format(time.RFC3339)
	body, err := json.MarshalIndent(status, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(jobsPath, status.JobID+".json"), body, 0644)
}

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

	dataPath := "/app/data"
	if _, err := os.Stat(dataPath); os.IsNotExist(err) {
		dataPath = "../data" // Fallback for local runs from gateway/
	}
	uploadPath := filepath.Join(dataPath, "raw")
	jobsPath := filepath.Join(dataPath, "jobs")
	outputPath := filepath.Join(dataPath, "output")
	os.MkdirAll(uploadPath, 0755)
	os.MkdirAll(jobsPath, 0755)
	os.MkdirAll(outputPath, 0755)

	r.Static("/output", outputPath)

	r.GET("/jobs/:id", func(c *gin.Context) {
		jobID := filepath.Base(c.Param("id"))
		if jobID == "." || jobID == string(filepath.Separator) || strings.Contains(jobID, "..") {
			c.JSON(http.StatusBadRequest, gin.H{"error": "Invalid job id"})
			return
		}

		body, err := os.ReadFile(filepath.Join(jobsPath, jobID+".json"))
		if err != nil {
			c.JSON(http.StatusNotFound, gin.H{"error": "Job not found"})
			return
		}

		c.Data(http.StatusOK, "application/json", body)
	})

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
		jobID := uuid.New().String()
		newFilename := jobID + ext
		dst := filepath.Join(uploadPath, newFilename)
		tmpDst := dst + ".part"

		if err := c.SaveUploadedFile(file, tmpDst); err != nil {
			logger.Error("save_failed", zap.String("filename", newFilename), zap.Error(err))
			c.JSON(http.StatusInternalServerError, gin.H{"error": fmt.Sprintf("Unable to save file: %s", err.Error())})
			return
		}

		// Save metadata if provided
		if metadata != "" {
			var metadataMap map[string]any
			if err := json.Unmarshal([]byte(metadata), &metadataMap); err == nil {
				metadataMap["job_id"] = jobID
				metadataMap["original_filename"] = file.Filename
				if enrichedMetadata, err := json.Marshal(metadataMap); err == nil {
					metadata = string(enrichedMetadata)
				}
			}

			metadataPath := filepath.Join(uploadPath, newFilename+"_metadata.json")
			if err := os.WriteFile(metadataPath, []byte(metadata), 0644); err != nil {
				logger.Error("metadata_save_failed", zap.String("filename", newFilename), zap.Error(err))
				// Don't fail the whole upload if metadata save fails
			} else {
				logger.Info("metadata_saved", zap.String("filename", newFilename))
			}
		}

		if err := writeJobStatus(jobsPath, jobStatus{
			JobID:            jobID,
			Status:           "uploaded",
			OriginalFilename: file.Filename,
			StoredFilename:   newFilename,
			Message:          "Upload complete. Waiting for analysis.",
			Progress:         10,
		}); err != nil {
			logger.Error("job_status_save_failed", zap.String("job_id", jobID), zap.Error(err))
		}

		if err := os.Rename(tmpDst, dst); err != nil {
			logger.Error("rename_failed", zap.String("filename", newFilename), zap.Error(err))
			_ = writeJobStatus(jobsPath, jobStatus{
				JobID:            jobID,
				Status:           "failed",
				OriginalFilename: file.Filename,
				StoredFilename:   newFilename,
				Message:          "Upload could not be finalized.",
				Progress:         100,
			})
			c.JSON(http.StatusInternalServerError, gin.H{"error": fmt.Sprintf("Unable to finalize file: %s", err.Error())})
			return
		}

		logger.Info("file_received",
			zap.String("job_id", jobID),
			zap.String("original_name", file.Filename),
			zap.String("stored_name", newFilename),
			zap.Bool("has_metadata", metadata != ""),
		)

		// 202 Accepted because processing happens asynchronously by other services
		c.JSON(http.StatusAccepted, gin.H{
			"message":  "File uploaded successfully",
			"job_id":   jobID,
			"filename": newFilename,
			"status":   "processing_started",
		})
	})

	logger.Info("service_starting", zap.String("port", "8080"))
	r.Run(":8080")
}
