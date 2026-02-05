package main

import (
	"encoding/json"
	"fmt"
	"html/template"
	"io"
	"net/http"
	"strings"
)

const (
	mediaMTXAPI  = "http://127.0.0.1:9997/v3/paths/list"
	mediaMTXHost = "stream.autumnal.de"
	listenAddr   = ":8080"
)

type PathItem struct {
	Name  string `json:"name"`
	Ready bool   `json:"ready"`
}

type MediaMTXResponse struct {
	Items []PathItem `json:"items"`
}

func getSubPaths(w http.ResponseWriter, r *http.Request) {
	basePath := r.URL.Query().Get("path")
	if basePath == "" {
		http.Error(w, "Path parameter missing", http.StatusBadRequest)
		return
	}

	resp, err := http.Get(mediaMTXAPI)
	if err != nil {
		http.Error(w, "MediaMTX unreachable", http.StatusServiceUnavailable)
		return
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)
	var mtxData MediaMTXResponse
	json.Unmarshal(body, &mtxData)

	var subPaths []string
	prefix := basePath + "/"
	for _, item := range mtxData.Items {
		if item.Ready && strings.HasPrefix(item.Name, prefix) {
			subPaths = append(subPaths, item.Name)
		}
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(subPaths)
}

func watchHandler(w http.ResponseWriter, r *http.Request) {
	requestedPath := strings.TrimPrefix(r.URL.Path, "/")
	if requestedPath == "" {
		http.Error(w, "Please specify a subpath", http.StatusBadRequest)
		return
	}

	tmpl, err := template.ParseFiles("index.html")
	if err != nil {
		http.Error(w, "Could not load index.html: "+err.Error(), http.StatusInternalServerError)
		return
	}

	data := struct {
		Path string
		Host string
	}{
		Path: requestedPath,
		Host: mediaMTXHost,
	}

	tmpl.Execute(w, data)
}

func main() {
	http.HandleFunc("/api/list", getSubPaths)
	http.HandleFunc("/", watchHandler)

	fmt.Printf("Web Portal running on %s\nAccess it at http://localhost%s/watch/your-path\n", listenAddr, listenAddr)
	http.ListenAndServe(listenAddr, nil)
}
