package niketsu_server

import (
	"flag"
	"log"
	"os"
	"sync"

	"github.com/BurntSushi/toml"
)

type PlaylistConfig struct {
	Playlist []string
	Video    *string
	Position *uint64
}

// TODO delete Name from config
type RoomConfig struct {
	SaveFile string
}

type General struct {
	Host     string
	Port     uint16
	Cert     *string
	Key      *string
	Password *string
	Debug    bool
}

type ServerConfig struct {
	General General
	Rooms   map[string]RoomConfig
}

var writeMutex sync.Mutex

func GetConfig() (General, map[string]*Room) {
	var configFile string
	flag.StringVar(&configFile, "config", "server/config.toml", "path to config file (toml)")
	flag.Parse()

	var serverConfig ServerConfig
	_, err := toml.DecodeFile(configFile, &serverConfig)
	if err != nil {
		log.Panicf("Failed to load config file at %s. Make sure the correct file format (toml) is used and the file exists", configFile)
	}

	rooms := make(map[string]*Room, 0)
	for name, roomConfig := range serverConfig.Rooms {
		var playlistConfig PlaylistConfig
		_, err = toml.DecodeFile(roomConfig.SaveFile, &playlistConfig)
		if err != nil {
			log.Printf("Playlist save file does not exist for room %s in %s. Using default values instead", name, roomConfig.SaveFile)
			newRoom := NewRoom(name, make([]string, 0), nil, nil, roomConfig.SaveFile)
			rooms[name] = &newRoom
		} else {
			newRoom := NewRoom(name, playlistConfig.Playlist, playlistConfig.Video, playlistConfig.Position, roomConfig.SaveFile)
			rooms[name] = &newRoom
		}
	}
	log.Printf("Configurations successfully set.\nServer configuration: %+v\nPlaylist Save File: %+v", serverConfig, rooms)

	return serverConfig.General, rooms
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

func DeleteConfig(saveFile string) {
	err := os.Remove(saveFile)
	if err != nil {
		logger.Warnw("Failed to delete playlist", saveFile)
	}
}
