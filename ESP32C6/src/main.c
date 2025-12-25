// for some reason, besides the system libraries, the io_mux_struct.h and gpio_struct.h header files target esp32c5 instead of esp32c6

#include <stdio.h>                // printf(), scanf(), file i/o, etc
#include <stdint.h>               // uint8_t, uint32_t, uintptr_t, intptr_t, etc
#include "esp_rom_sys.h"          // esp_rom_delay_us()
#include "soc/gpio_struct.h"      // GPIO
#include "soc/i2c_struct.h"       // I2C
//#include "soc/gpio_reg.h"         //This is GPIO matrix router, will probably be used later but not needed yet

#define LED_GPIO   0

static inline void led_init(void){ // static means function only accessible within this file. multiple files can have "init" function, when refactoring using static means this function only confined to this file so it wont break elsewhere
                                     // inline, the compiler replaces a function call with the function's actual code, used for small functions and surgically
    
    GPIO.enable_w1ts.val |= (1U << LED_GPIO); // enable GPIO0 // there's the output register which means any change overwrites the whole thing, output set, and output clear. Output register just writes the whole register, output set sets specific bits, output clear clears specific bits and GPIO 0-31, 32-32 beacause there isnt anything after 32
    GPIO.out_w1tc.val = (1U << LED_GPIO);    // set GPIO0 low
}

static inline void i2c_init(void){

}

void app_main(void){
    led_init();
    while(1){
        //heartbeat
        GPIO.out_w1ts.val = (1U << LED_GPIO);
        esp_rom_delay_us(500000);
        GPIO.out_w1tc.val = (1U << LED_GPIO);
        esp_rom_delay_us(500000);
    }
}

/*
#include <inttypes.h>
#include "sdkconfig.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_chip_info.h"
#include "esp_flash.h"
#include "esp_system.h"

void app_main(void)
{
    printf("Hello world!\n");

     Print chip information 
    esp_chip_info_t chip_info;
    uint32_t flash_size;
    esp_chip_info(&chip_info);
    printf("This is %s chip with %d CPU core(s), %s%s%s%s, ",
           CONFIG_IDF_TARGET,
           chip_info.cores,
           (chip_info.features & CHIP_FEATURE_WIFI_BGN) ? "WiFi/" : "",
           (chip_info.features & CHIP_FEATURE_BT) ? "BT" : "",
           (chip_info.features & CHIP_FEATURE_BLE) ? "BLE" : "",
           (chip_info.features & CHIP_FEATURE_IEEE802154) ? ", 802.15.4 (Zigbee/Thread)" : "");

    unsigned major_rev = chip_info.revision / 100;
    unsigned minor_rev = chip_info.revision % 100;
    printf("silicon revision v%d.%d, ", major_rev, minor_rev);
    if(esp_flash_get_size(NULL, &flash_size) != ESP_OK) {
        printf("Get flash size failed");
        return;
    }

    printf("%" PRIu32 "MB %s flash\n", flash_size / (uint32_t)(1024 * 1024),
           (chip_info.features & CHIP_FEATURE_EMB_FLASH) ? "embedded" : "external");

    printf("Minimum free heap size: %" PRIu32 " bytes\n", esp_get_minimum_free_heap_size());

    for (int i = 10; i >= 0; i--) {
        printf("Restarting in %d seconds...\n", i);
        vTaskDelay(1000 / portTICK_PERIOD_MS);
    }
    printf("Restarting now.\n");
    fflush(stdout);
    esp_restart();
}
*/