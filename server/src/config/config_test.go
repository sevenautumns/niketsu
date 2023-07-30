package config

import (
	"encoding/json"
	"fmt"
	"os"
	"reflect"
	"strconv"
	"strings"
	"testing"

	"github.com/stretchr/testify/require"
)

var (
	emptyConfig   = CLI{}
	defaultConfig = CLI{
		Config:           "",
		Host:             "someHost",
		Port:             1111,
		Cert:             "someCert",
		Key:              "someKey",
		Password:         "somePW",
		DBPath:           ".db/",
		DBUpdateInterval: 10,
		DBWaitTimeout:    4,
		Debug:            true,
	}
	fileConfig = CLI{
		Config:           "",
		Host:             "0.0.0.0",
		Port:             1111,
		DBPath:           "somedb/",
		Debug:            true,
		Cert:             "cert.pem",
		Key:              "key.pem",
		Password:         "1234",
		DBUpdateInterval: 1,
		DBWaitTimeout:    1,
	}
	fileOnlyConfig = CLI{
		Config: "testdata/config.json",
	}
	fileHalfConfig = CLI{
		Config: "",
		Host:   "1.1.1.1",
		Port:   2222,
		DBPath: ".someotherdb/",
	}
	fileOnlyHalfConfig = CLI{
		Config: "testdata/half_config.json",
	}
	halfConfig = CLI{
		Password: "someOther",
		Key:      "some.key",
		Cert:     "some.cert",
	}
	fileAllConfig = CLI{
		Config:           "testdata/config.json",
		Host:             "0.0.0.0",
		Port:             1111,
		DBPath:           "somedb/",
		Debug:            true,
		Cert:             "cert.pem",
		Key:              "key.pem",
		Password:         "1234",
		DBUpdateInterval: 1,
		DBWaitTimeout:    1,
	}
	fileHalfAllConfig = CLI{
		Config:   "testdata/half_config.json",
		Host:     "2.2.2.2",
		Password: "1234",
	}
)

func TestParseConfig(t *testing.T) {
	setArgs(defaultConfig)
	config := ParseCommandArgs()
	testConfigsEqual(t, defaultConfig, config)
}

func setArgs(config CLI) {
	values := reflect.ValueOf(config)
	types := values.Type()
	args := []string{"go config_test.go"}
	for i := 0; i < types.NumField(); i++ {
		name := strings.ToLower(types.Field(i).Name)
		val := values.Field(i).Interface()
		if isEmpty(val) {
			continue
		}
		var field string
		switch val.(type) {
		case bool:
			boolVal := val.(bool)
			if boolVal {
				field = fmt.Sprintf("--%s", name)
			}
		default:
			field = fmt.Sprintf("--%s=%v", name, val)
		}
		args = append(args, field)
	}

	os.Args = args
}

func testConfigsEqual(t *testing.T, expectedConfig CLI, actualConfig CLI) {
	require.Equal(t, expectedConfig.Host, actualConfig.Host)
	require.Equal(t, expectedConfig.Port, actualConfig.Port)
	require.Equal(t, expectedConfig.Cert, actualConfig.Cert)
	require.Equal(t, expectedConfig.Key, actualConfig.Key)
	require.Equal(t, expectedConfig.Password, actualConfig.Password)
	require.Equal(t, expectedConfig.DBPath, actualConfig.DBPath)
	require.Equal(t, expectedConfig.DBUpdateInterval, actualConfig.DBUpdateInterval)
	require.Equal(t, expectedConfig.DBWaitTimeout, actualConfig.DBWaitTimeout)
	require.Equal(t, expectedConfig.Debug, actualConfig.Debug)
}

func isEmpty(x interface{}) bool {
	return reflect.DeepEqual(x, reflect.Zero(reflect.TypeOf(x)).Interface())
}

func TestParseConfigWithEnvVars(t *testing.T) {
	resetOsArgs()
	setEnvVars(defaultConfig)
	config := ParseCommandArgs()
	testConfigsEqual(t, defaultConfig, config)
}

func resetOsArgs() {
	os.Args = []string{"go config_test.go"}
}

func setEnvVars(config CLI) {
	values := reflect.ValueOf(config)
	types := values.Type()

	for i := 0; i < types.NumField(); i++ {
		name := strings.ToUpper(types.Field(i).Name)
		val := values.Field(i).Interface()
		var field string
		switch val.(type) {
		case bool:
			field = strconv.FormatBool(val.(bool))
		case string:
			field = val.(string)
		case uint16:
			field = strconv.FormatUint(uint64(val.(uint16)), 10)
		case uint64:
			field = strconv.FormatUint(val.(uint64), 10)
		default:
			continue
		}
		os.Setenv(name, field)
	}
}

func TestGetConfigFromFile(t *testing.T) {
	resetOsArgs()
	createConfigFile(t, defaultConfig, niketsuProjectPath)
	config := ParseCommandArgs()
	testConfigsEqual(t, defaultConfig, config)
}

func createConfigFile(t *testing.T, cli CLI, path string) {
	file, _ := os.OpenFile(path, os.O_CREATE|os.O_WRONLY, os.ModePerm)
	defer file.Close()

	encoder := json.NewEncoder(file)
	err := encoder.Encode(cli)
	require.NoError(t, err)

	t.Cleanup(func() {
		os.Remove(path)
	})
}

func TestGetConfigFromGivenFile(t *testing.T) {
	setArgs(fileOnlyConfig)
	config := ParseCommandArgs()
	testConfigsEqual(t, fileConfig, config)
}

func TestGetConfigFromFileWithEnvVars(t *testing.T) {
	setArgs(fileOnlyConfig)
	setEnvVars(halfConfig)
	config := ParseCommandArgs()
	testConfigsEqual(t, fileConfig, config)
}

func TestGetSomeConfigFromFileWithEnvVars(t *testing.T) {
	setArgs(fileOnlyHalfConfig)
	setEnvVars(defaultConfig)
	config := ParseCommandArgs()
	require.Equal(t, fileOnlyHalfConfig.Config, config.Config)
	require.Equal(t, fileHalfConfig.Port, config.Port)
	require.Equal(t, fileHalfConfig.Host, config.Host)
	require.Equal(t, fileHalfConfig.DBPath, config.DBPath)
	require.Equal(t, defaultConfig.Debug, config.Debug)
	require.Equal(t, defaultConfig.Cert, config.Cert)
	require.Equal(t, defaultConfig.Key, config.Key)
	require.Equal(t, defaultConfig.Password, config.Password)
	require.Equal(t, defaultConfig.DBUpdateInterval, config.DBUpdateInterval)
	require.Equal(t, defaultConfig.DBWaitTimeout, config.DBWaitTimeout)
}

func TestGetConfigFromFileWithCommandArgs(t *testing.T) {
	setArgs(defaultConfig)
	setEnvVars(fileConfig)
	config := ParseCommandArgs()
	testConfigsEqual(t, defaultConfig, config)
}

func TestGetConfigFromFileWithSomeCommandArgs(t *testing.T) {
	setArgs(halfConfig)
	setEnvVars(fileConfig)
	config := ParseCommandArgs()
	require.Equal(t, fileConfig.Config, config.Config)
	require.Equal(t, fileConfig.Port, config.Port)
	require.Equal(t, fileConfig.Host, config.Host)
	require.Equal(t, fileConfig.DBPath, config.DBPath)
	require.Equal(t, fileConfig.Debug, config.Debug)
	require.Equal(t, halfConfig.Cert, config.Cert)
	require.Equal(t, halfConfig.Key, config.Key)
	require.Equal(t, halfConfig.Password, config.Password)
	require.Equal(t, fileConfig.DBUpdateInterval, config.DBUpdateInterval)
	require.Equal(t, fileConfig.DBWaitTimeout, config.DBWaitTimeout)
}

func TestGetConfigFromFileWithEnvVarsAndCommandArgs(t *testing.T) {
	setArgs(fileAllConfig)
	setEnvVars(defaultConfig)
	config := ParseCommandArgs()
	testConfigsEqual(t, fileAllConfig, config)
}

func TestGetSomeConfigFromFileWithSomeEnvVarsAndSomeCommandArgs(t *testing.T) {
	setArgs(fileHalfAllConfig)
	setEnvVars(defaultConfig)
	config := ParseCommandArgs()
	require.Equal(t, fileHalfAllConfig.Config, config.Config)
	require.Equal(t, fileHalfConfig.Port, config.Port)
	require.Equal(t, fileHalfAllConfig.Host, config.Host)
	require.Equal(t, fileHalfConfig.DBPath, config.DBPath)
	require.Equal(t, defaultConfig.Debug, config.Debug)
	require.Equal(t, defaultConfig.Cert, config.Cert)
	require.Equal(t, defaultConfig.Key, config.Key)
	require.Equal(t, fileHalfAllConfig.Password, config.Password)
	require.Equal(t, defaultConfig.DBUpdateInterval, config.DBUpdateInterval)
	require.Equal(t, defaultConfig.DBWaitTimeout, config.DBWaitTimeout)
}
