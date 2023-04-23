package niketsu_server

import (
	"flag"
	"log"
	"os"
	"sync"

	"github.com/BurntSushi/toml"
)

type (
	ServerConfig struct {
		Host     string
		Port     uint16
		SaveFile string
		Debug    bool
	}

	PlaylistConfig struct {
		Playlist []string
		Video    *string
		Position *uint64
	}
)

var writeMutex sync.Mutex

func GetConfig() (ServerConfig, PlaylistConfig) {
	var configFile string
	flag.StringVar(&configFile, "config", "server/config.toml", "path to config file (toml)")
	flag.Parse()

	var serverConfig ServerConfig
	_, err := toml.DecodeFile(configFile, &serverConfig)
	if err != nil {
		log.Panicf("Failed to load config file at %s. Make sure the correct file format (toml) is used and the file exists", configFile)
	}

	var playlistConfig PlaylistConfig
	_, err = toml.DecodeFile(serverConfig.SaveFile, &playlistConfig)
	if err != nil {
		log.Printf("Playlist save file does not exist at %s. Using default values instead", serverConfig.SaveFile)
		playlistConfig = PlaylistConfig{Playlist: make([]string, 0), Video: nil, Position: nil}
	}

	log.Printf("Configurations successfully set.\nServer configuration: %+v\nPlaylist Save File: %+v", serverConfig, playlistConfig)

	return serverConfig, playlistConfig
}

func WritePlaylist(playlist []string, video *string, position *uint64, saveFile string) {
	writeMutex.Lock()
	defer writeMutex.Unlock()

	playlistConfig := PlaylistConfig{Playlist: playlist, Video: video, Position: position}

	f, err := os.Create(saveFile)
	if err != nil {
		log.Fatal("Failed to open or create toml save file for playlist")
	}

	if err := toml.NewEncoder(f).Encode(playlistConfig); err != nil {
		logger.Warn("Failed to write toml save file for playlist")
	}

	if err := f.Close(); err != nil {
		logger.Warn("Failed to close toml save file for playlist")
	}
}
